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

use std::sync::atomic::Ordering;
use std::sync::Mutex;

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

/// Install the hook and start the thread that acts on it.
///
/// If the hook cannot be installed this logs and returns: Alt+Tab is then Windows'
/// own, which is the required fallback rather than an error.
pub fn start(app: AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<win::hotkeys::Key>();
    if !win::hotkeys::install(tx) {
        return;
    }

    std::thread::spawn(move || {
        while let Ok(key) = rx.recv() {
            match key {
                win::hotkeys::Key::Switch(backwards) => advance(&app, backwards),
                win::hotkeys::Key::Commit => commit(&app),
                win::hotkeys::Key::Cancel => cancel(&app),
            }
        }
    });
}

fn build_entries() -> Vec<Entry> {
    win::apps::list_windows()
        .into_iter()
        .map(|w| Entry {
            // The icon comes from the owning executable, the same path the dock
            // uses, so a window and its dock button always show the same picture.
            icon: w
                .exe_path
                .as_ref()
                .and_then(|p| win::icons::icon_data_url(&p.to_string_lossy())),
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
        return;
    }

    // Windows selects the SECOND entry: the window you were in before this one.
    let selected = if backwards { entries.len() - 1 } else { 1 };
    let payload = (entries.clone(), selected);
    *guard = Some(State { entries, selected });
    drop(guard);

    win::hotkeys::SWITCHER_OPEN.store(true, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("switcher") {
        let _ = app.emit("switcher:show", payload);
        let _ = w.show();
    } else {
        crate::logln!("switcher window missing; cancelling");
        close(app);
    }
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
    win::hotkeys::SWITCHER_OPEN.store(false, Ordering::SeqCst);
    *STATE.lock().unwrap() = None;
    win::dwm::clear();
    if let Some(w) = app.get_webview_window("switcher") {
        let _ = w.hide();
    }
}

/// Register a DWM preview for each card the page laid out.
pub fn place_thumbnails(dest: isize, slots: &[crate::SwitcherSlot], scale: f64) {
    // Previous registrations first: the page reports slots again every time the
    // switcher opens, and a leaked thumbnail holds a reference to a window that may
    // be on its way out.
    win::dwm::clear();
    for s in slots {
        win::dwm::show(
            dest,
            s.hwnd,
            (s.x * scale).round() as i32,
            (s.y * scale).round() as i32,
            (s.w * scale).round() as i32,
            (s.h * scale).round() as i32,
        );
    }
}
