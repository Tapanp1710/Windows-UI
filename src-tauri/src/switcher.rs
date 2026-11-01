//! The Alt+Tab switcher: the state machine behind the keyboard hook.
//!
//! The hook (win/hotkeys.rs) only decides whether to swallow a keystroke; everything
//! that takes time happens here, on a thread of its own. That split is what keeps the
//! hook fast enough that Windows never drops it.
//!
//! Behaviour copied from Windows exactly, because the core rule says it must be:
//!   - Alt+Tab opens the switcher with the **second** window selected, i.e. the one
//!     you were in before this one. Tapping Alt+Tab and letting go swaps the two.
//!   - Holding Alt and pressing Tab again moves forward through the list; Shift+Tab
//!     moves back. Both wrap around.
//!   - Releasing Alt switches to the selection. Escape cancels and leaves you where
//!     you were.
//!   - The order is the z-order, which is Windows' own most-recently-used order.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::win;

/// The windows the switcher is currently offering, in order, with the selection.
struct State {
    entries: Vec<Entry>,
    selected: usize,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub hwnd: isize,
    pub title: String,
    pub icon: Option<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Bumped every time the switcher opens or closes. A page render, a thumbnail
/// placement or a fade that belongs to an older opening sees a different number
/// and does nothing, so nothing late can put a closed switcher back on screen.
static GEN: AtomicU64 = AtomicU64::new(0);

/// The last generation the page finished laying out, and a way to wait for it.
static RENDERED: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

/// Current opacity of the previews, so ones placed mid-fade join in at the right
/// level.
static THUMB_ALPHA: AtomicU8 = AtomicU8::new(0);

/// Icons by executable path. Extracting and encoding a 256px icon per window on
/// every Alt+Tab was most of the delay before the switcher appeared.
static ICONS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

/// How long the switcher waits for the page to lay out before showing anyway.
const RENDER_WAIT: Duration = Duration::from_millis(150);

/// While open, how often the physical Alt key is checked.
const ALT_POLL: Duration = Duration::from_millis(40);

/// Fade-in for the previews, matched to the bar's CSS fade.
const FADE_STEPS: u32 = 7;
const FADE_STEP: Duration = Duration::from_millis(16);

/// Install the hook and start the thread that acts on it.
///
/// If the hook cannot be installed this logs and returns: Alt+Tab is then Windows'
/// own, which is the required fallback rather than an error.
pub fn start(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<win::hotkeys::Key>();
    if !win::hotkeys::install(tx) {
        return;
    }
    // Covers the Win presses the hook cannot see - see the module comment there.
    win::hotkeys::start_guard();

    // Fill the icon cache now, so even the first Alt+Tab opens at full speed.
    std::thread::spawn(|| {
        let _ = build_entries();
    });

    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;
        loop {
            // Copied out on its own line: no guard may outlive this check (D-056).
            let open = STATE.lock().unwrap().is_some();
            let key = if open {
                // A missed Alt release must never strand the switcher on screen.
                // The hook can miss one - an elevated window gets the key, or
                // Windows drops a slow hook - so the key itself is the authority.
                match rx.recv_timeout(ALT_POLL) {
                    Ok(k) => k,
                    Err(RecvTimeoutError::Timeout) => {
                        if !win::hotkeys::alt_held() {
                            crate::logln!("switcher: Alt is up without a release event; committing");
                            commit(&app);
                        }
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(k) => k,
                    Err(_) => break,
                }
            };
            match key {
                win::hotkeys::Key::Switch(backwards) => advance(&app, backwards),
                win::hotkeys::Key::Commit => commit(&app),
                win::hotkeys::Key::Cancel => cancel(&app),
                // The launcher shares this hook and therefore this thread; both are
                // just "show a window", so there is nothing to gain from a second.
                win::hotkeys::Key::Launcher => crate::launcher::toggle(&app),
                win::hotkeys::Key::CloseLauncher => crate::launcher::close(&app),
                win::hotkeys::Key::OpenLauncher => crate::launcher::open(&app),
            }
        }
    });
}

fn icon_for(exe: &std::path::Path) -> Option<String> {
    let key = exe.to_string_lossy().to_string();
    let cache = ICONS.get_or_init(|| Mutex::new(HashMap::new()));
    let hit = cache.lock().unwrap().get(&key).cloned();
    if let Some(icon) = hit {
        return icon;
    }
    // Extracted with no lock held: this is the slow part.
    let icon = win::icons::icon_data_url(&key);
    cache.lock().unwrap().insert(key, icon.clone());
    icon
}

fn build_entries() -> Vec<Entry> {
    win::apps::list_windows()
        .into_iter()
        .map(|w| Entry {
            // The icon comes from the owning executable, the same path the dock
            // uses, so a window and its dock button always show the same picture.
            icon: w.exe_path.as_deref().and_then(icon_for),
            hwnd: w.hwnd,
            title: w.title,
        })
        .collect()
}

fn advance(app: &AppHandle, backwards: bool) {
    let mut guard = STATE.lock().unwrap();

    match guard.as_mut() {
        // Already open: just move the selection.
        Some(state) if !state.entries.is_empty() => {
            let n = state.entries.len();
            state.selected = if backwards {
                (state.selected + n - 1) % n
            } else {
                (state.selected + 1) % n
            };
            let _ = app.emit("switcher:select", state.selected);
            return;
        }
        _ => {}
    }

    let entries = build_entries();
    if entries.len() < 2 {
        // Nothing to switch between. Leave the switcher closed rather than showing a
        // panel with one entry - and because the Tab was swallowed, do the obvious
        // thing and let Windows have this one next time.
        crate::logln!("Alt+Tab with {} window(s); nothing to show", entries.len());
        win::hotkeys::SWITCHER_OPEN.store(false, Ordering::SeqCst);
        return;
    }

    // Windows selects the SECOND entry: the window you were in before this one.
    let selected = if backwards { entries.len() - 1 } else { 1 };
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let payload = (entries.clone(), selected, gen);
    *guard = Some(State { entries, selected });
    drop(guard);

    // A quick tap - Alt already back up - is a straight swap, as in Windows: the
    // Commit is already queued behind this, so there is nothing to show.
    if !win::hotkeys::alt_held() {
        return;
    }

    let Some(w) = app.get_webview_window("switcher") else {
        crate::logln!("switcher window missing; cancelling");
        close(app);
        return;
    };

    // Let the page lay out before the window appears, so it never shows the last
    // opening's cards for a frame.
    THUMB_ALPHA.store(0, Ordering::SeqCst);
    let _ = app.emit("switcher:show", payload);
    {
        let (lock, cv) = &RENDERED;
        let done = lock.lock().unwrap();
        let _ = cv.wait_timeout_while(done, RENDER_WAIT, |r| *r < gen);
    }
    if !win::hotkeys::alt_held() || GEN.load(Ordering::SeqCst) != gen {
        return;
    }
    let _ = w.show();
    let _ = app.emit("switcher:visible", gen);
    fade_in(gen);
}

/// Bring the previews up alongside the bar's CSS fade.
fn fade_in(gen: u64) {
    if !win::animations_enabled() {
        THUMB_ALPHA.store(255, Ordering::SeqCst);
        win::dwm::set_opacity(255);
        return;
    }
    std::thread::spawn(move || {
        for i in 1..=FADE_STEPS {
            std::thread::sleep(FADE_STEP);
            if GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            // Ease-out, so the previews arrive quickly and settle.
            let t = i as f32 / FADE_STEPS as f32;
            let a = (255.0 * (1.0 - (1.0 - t).powi(3))).round() as u8;
            THUMB_ALPHA.store(a, Ordering::SeqCst);
            win::dwm::set_opacity(a);
        }
    });
}

fn commit(app: &AppHandle) {
    let target = STATE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|s| s.entries.get(s.selected).map(|e| e.hwnd));
    close(app);
    if let Some(hwnd) = target {
        win::apps::activate(hwnd);
    }
}

fn cancel(app: &AppHandle) {
    close(app);
}

/// A click on one of the cards.
pub fn pick(app: &AppHandle, hwnd: isize) {
    close(app);
    win::apps::activate(hwnd);
}

fn close(app: &AppHandle) {
    GEN.fetch_add(1, Ordering::SeqCst);
    win::hotkeys::SWITCHER_OPEN.store(false, Ordering::SeqCst);
    *STATE.lock().unwrap() = None;
    THUMB_ALPHA.store(0, Ordering::SeqCst);
    win::dwm::clear();
    if let Some(w) = app.get_webview_window("switcher") {
        let _ = w.hide();
    }
    let _ = app.emit("switcher:hide", ());
}

/// Register a DWM preview for each card the page laid out.
///
/// Called for the first layout and again whenever the cards scroll. Stale
/// generations are ignored: the switcher has closed or reopened since.
pub fn place_thumbnails(dest: isize, slots: &[crate::SwitcherSlot], scale: f64, gen: u64) {
    if GEN.load(Ordering::SeqCst) != gen {
        return;
    }
    // Previous registrations first: a leaked thumbnail holds a reference to a
    // window that may be on its way out.
    win::dwm::clear();
    let alpha = THUMB_ALPHA.load(Ordering::SeqCst);
    for s in slots {
        win::dwm::show(
            dest,
            s.hwnd,
            (s.x * scale).round() as i32,
            (s.y * scale).round() as i32,
            (s.w * scale).round() as i32,
            (s.h * scale).round() as i32,
            alpha,
        );
    }
    let (lock, cv) = &RENDERED;
    *lock.lock().unwrap() = gen;
    cv.notify_all();
}
