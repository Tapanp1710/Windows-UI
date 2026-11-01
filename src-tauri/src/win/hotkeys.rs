//! The one keyboard hook in this project, and the rules it lives by.
//!
//! Alt+Tab cannot be observed any other way: the shell reserves it, so a
//! `WH_KEYBOARD_LL` hook is the only way to put something else on it. Everything else
//! in this project deliberately avoids hooks (see autohide.rs), so this one is kept
//! as small and as inert as possible.
//!
//! **What it swallows, and nothing else:**
//!   - `Tab` while Alt is physically held — the whole point.
//!   - `Esc` while the switcher or the launcher is open — cancelling.
//!   - The Windows key's **release**, and only when nothing else was pressed while it
//!     was held. See the note on the Win key below.
//!
//! It never swallows Alt itself. Letting Alt through means the window you are
//! leaving sees a complete Alt press rather than a half one, so it cannot be left
//! with a stuck modifier; the cost is that an app with a menu bar may flash it as you
//! switch away. A stuck Alt would be far worse than a flicker.
//!
//! **Every way this can fail leaves Windows' own Alt+Tab working:**
//!   - `SetWindowsHookEx` fails → nothing is installed, nothing is swallowed.
//!   - An elevated window has focus → Windows does not deliver our hook its input at
//!     all, so the native switcher runs. This is automatic, not something we detect.
//!   - The callback takes too long → Windows silently drops the hook, which again
//!     restores the native switcher.
//!   - The process dies, however it dies → the hook dies with it.
//!
//! ## The Win key
//!
//! Windows opens Start on the Win key's **release**, and only if nothing else was
//! pressed in between. That is the hook that matters: the key*down* is passed through
//! untouched, so `Win+E`, `Win+R`, `Win+L`, `Win+Shift+S`, `Win+1` and every other
//! combination reach Windows exactly as they always did — this never sees them as
//! anything but "something else was pressed".
//!
//! Only the release of a Win key that was pressed and let go **alone** is swallowed,
//! which is precisely the keystroke that would have opened Start. Swallowing the
//! key*down* instead would have meant re-synthesising it for every combination, from
//! inside the hook callback, with the combination's own keystroke already in flight.
//! That is the version of this that breaks Win+L.
//!
//! A mouse click while Win is held also counts as "used", because Windows treats it
//! that way too.
//!
//! The callback therefore does the least work that can possibly answer "swallow or
//! not": it reads a couple of key states and pushes an enum down a channel. It never
//! touches a window, never allocates, and never blocks.
//!
//! ## The guard
//!
//! The hook alone still let Windows Start open on some presses in real use (D-059).
//! It is blind while one of our own windows has focus, and Windows silently drops a
//! low-level hook that is ever slow to answer, after which every press went to
//! Start until a restart. So a second, passive check runs beside it: `start_guard`
//! reads the Win key's physical state (a read, like the dock's cursor sampling -
//! nothing is installed and nothing is swallowed) and covers any press the hook did
//! not see. It masks that press the same way, puts the hook back if it has been
//! dropped, and as a last resort closes Start if it opens anyway.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// What the hook saw, handed to the UI thread to act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    /// Alt+Tab, or Alt+Shift+Tab when `true`.
    Switch(bool),
    /// Escape while the switcher is open.
    Cancel,
    /// Alt released while the switcher is open: commit the selection.
    Commit,
    /// The Win key was pressed and released on its own.
    Launcher,
    /// Escape while the launcher is open.
    CloseLauncher,
    /// Windows Start opened in place of the launcher and has been closed: show ours.
    OpenLauncher,
}

static TX: OnceLock<Sender<Key>> = OnceLock::new();
static HOOK: AtomicIsize = AtomicIsize::new(0);
/// The thread pumping the hook, so a replacement hook can retire it.
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// Counts the Win presses the hook has seen, so the guard can tell whether a press
/// was the hook's to handle.
static PRESS_SEQ: AtomicU64 = AtomicU64::new(0);
/// When we last injected a Ctrl tap, in ms since start, so the guard does not take
/// our own Ctrl for the user pressing a combination.
static LAST_INJECT_MS: AtomicU64 = AtomicU64::new(0);

/// Whether the switcher is on screen. Read by the hook to decide whether Esc and the
/// Alt release mean anything, so it must be kept in step by the UI side.
pub static SWITCHER_OPEN: AtomicBool = AtomicBool::new(false);

/// Master switch for the switcher. When false the hook stops swallowing Tab, which
/// is the setting-level way to hand Alt+Tab back to Windows.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Master switch for the launcher. When false the Win key is never touched and
/// Windows Start opens exactly as it always did.
pub static LAUNCHER_ENABLED: AtomicBool = AtomicBool::new(true);
/// Whether the launcher is on screen, so Escape means something.
pub static LAUNCHER_OPEN: AtomicBool = AtomicBool::new(false);

/// True while a Win key is physically held.
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
/// True once anything else happened during this Win press, which means the release
/// must be passed straight through.
static WIN_USED: AtomicBool = AtomicBool::new(false);

fn held(vk: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

/// Whether either Alt key is physically down right now.
pub fn alt_held() -> bool {
    held(windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU.0 as i32)
}

/// Set while three fingers are on the touchpad holding the switcher open - the
/// touchpad's equivalent of a held Alt (win/touchpad.rs).
pub static GESTURE_HELD: AtomicBool = AtomicBool::new(false);

/// Whether the switcher is being held open: Alt is down, or a three-finger swipe is
/// still on the pad. A gesture that has gone quiet for a second no longer counts,
/// so a lost "fingers lifted" report cannot leave the switcher on screen.
pub fn switch_held() -> bool {
    alt_held()
        || (GESTURE_HELD.load(Ordering::SeqCst)
            && now_ms().saturating_sub(crate::win::touchpad::LAST_FRAME_MS.load(Ordering::Relaxed)) < 1000)
}

/// Let the Win key up **without** opening Start.
///
/// Start opens on the Win key's release, and only when nothing was pressed during
/// the hold. D-044 is the record of what happens if you simply swallow that release:
/// Windows is never told the modifier came back up, it stays latched system-wide,
/// and every later keystroke becomes a chord. The key has to come up.
///
/// So the real release is swallowed and three events are injected in its place:
/// Control down, Control up, then Win up. Windows sees the press as part of a
/// chord - which it ignores, Win+Ctrl alone does nothing - so Start stays shut, and
/// the final Win up clears the latch properly.
///
/// Order matters: these are queued behind the event being handled, so they must be
/// injected *instead of* passing the real release on, never alongside it. All three
/// carry LLKHF_INJECTED and are skipped at the top of the hook.
/// Marks keystrokes this module injects, so the hook can skip its own and nobody
/// else's. Arbitrary, just has to be unlikely: "macu" in ASCII.
const INJECTED_BY_US: usize = 0x6D61_6375;

unsafe fn release_win_without_start(win_vk: u16) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
    };

    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_BY_US,
            },
        },
    };

    LAST_INJECT_MS.store(now_ms(), Ordering::Relaxed);
    let events = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
        key(VIRTUAL_KEY(win_vk), KEYEVENTF_KEYUP),
    ];
    SendInput(&events, std::mem::size_of::<INPUT>() as i32);
}

/// Make a Win press that is still held count as a combination, so its release
/// does not open Start.
///
/// For the guard, on a press the hook did not see. While one of our windows has
/// keyboard focus this hook is not called at all (measured: with the launcher
/// focused, Windows' own Alt+Tab appeared and no Win release reached the hook).
/// A Ctrl tap while Win is down is exactly
/// what Windows treats as "something else was pressed", and nothing is swallowed:
/// the real Win release still goes through, so the modifier cannot latch (D-044).
fn mask_held_win() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VK_CONTROL,
    };
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_CONTROL,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_BY_US,
            },
        },
    };
    LAST_INJECT_MS.store(now_ms(), Ordering::Relaxed);
    unsafe {
        SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_LMENU, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN, VK_SHIFT, VK_TAB,
    };

    // HC_ACTION is 0. Anything else must be passed on untouched.
    if code == 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);

        // Skip **our own** injections, not every injected event.
        //
        // `release_win_without_start` synthesises keystrokes that come straight back
        // through this hook, and treating them as real input would mark the press
        // "used" and loop. The obvious test is LLKHF_INJECTED - and it is wrong:
        // that flag is set by on-screen keyboards, remote desktop, accessibility
        // tools and macro software too, so keying off it means the launcher quietly
        // stops working for anyone typing through one of those. Tagging our own
        // events with `dwExtraInfo` identifies exactly ours and nothing else.
        if kb.dwExtraInfo == INJECTED_BY_US {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let vk = kb.vkCode as u16;
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;

        // --- the Win key ------------------------------------------------------
        // Tracked on every event, whatever the master switches say, so the state
        // can never go stale and strand a half-finished press.
        if is_win && down {
            // Only the FIRST keydown of a hold starts a fresh press. Holding Win
            // repeats the keydown, and clearing "used" on each repeat would make
            // Win+<held key> look like a bare Win press on release.
            if !WIN_DOWN.swap(true, Ordering::Relaxed) {
                WIN_USED.store(false, Ordering::Relaxed);
                PRESS_SEQ.fetch_add(1, Ordering::Relaxed);
            }
        } else if down && WIN_DOWN.load(Ordering::Relaxed) {
            // Something else was pressed with Win held: this is a combination, and
            // its release must go through untouched.
            WIN_USED.store(true, Ordering::Relaxed);
        }

        if is_win && up {
            let used = WIN_USED.swap(false, Ordering::Relaxed)
                // Windows counts a click during the press as "used" too.
                || crate::win::mouse_button_down();
            WIN_DOWN.store(false, Ordering::Relaxed);

            if !used && LAUNCHER_ENABLED.load(Ordering::Relaxed) {
                // Swallow this release and hand Windows a different one - see
                // `release_win_without_start` and D-052. The key MUST still come
                // up as far as Windows is concerned; D-044 is what happens when
                // it does not.
                release_win_without_start(vk);
                send(Key::Launcher);
                return LRESULT(1);
            }
        }

        if down && vk == 0x1B && LAUNCHER_OPEN.load(Ordering::Relaxed) {
            send(Key::CloseLauncher);
            return LRESULT(1);
        }
    }

    if code == 0 && ENABLED.load(Ordering::Relaxed) {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode as u16;
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        if down && vk == VK_TAB.0 && held(VK_MENU.0 as i32) {
            // Marked open here, in the hook, not when the switcher thread gets round
            // to showing the window. Building the list takes a moment, and an Alt
            // released inside it used to find the flag still false: no Commit was
            // ever sent and the switcher stayed on screen with Alt up (D-057).
            SWITCHER_OPEN.store(true, Ordering::SeqCst);
            send(Key::Switch(held(VK_SHIFT.0 as i32)));
            // Swallowed: this is the only keystroke this project takes away from
            // Windows, and only while Alt is physically down.
            return LRESULT(1);
        }

        if SWITCHER_OPEN.load(Ordering::Relaxed) {
            if down && vk == 0x1B {
                send(Key::Cancel);
                return LRESULT(1);
            }
            if up && (vk == VK_LMENU.0 || vk == VK_RMENU.0 || vk == VK_MENU.0) {
                // Deliberately NOT swallowed - see the module comment. Cleared here
                // so a second Alt release cannot send a second Commit.
                SWITCHER_OPEN.store(false, Ordering::SeqCst);
                send(Key::Commit);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

/// For the touchpad, which drives the switcher through the same channel.
pub fn send_key(key: Key) {
    send(key);
}

fn send(key: Key) {
    if let Some(tx) = TX.get() {
        // try_send semantics: a full or disconnected channel must never block the
        // input path. Dropping an event loses one keystroke; blocking here would
        // freeze typing for the whole machine.
        let _ = tx.send(key);
    }
}

/// Install the hook on a thread of its own with its own message loop.
///
/// A low-level hook is delivered to the thread that installed it, and that thread
/// must be pumping messages. Giving it a dedicated thread keeps hook delivery off
/// the UI thread entirely, so a busy webview can never make the hook time out and be
/// dropped.
///
/// Returns false if the hook could not be installed, in which case nothing is
/// swallowed and Windows' own Alt+Tab is untouched.
pub fn install(tx: Sender<Key>) -> bool {
    let _ = TX.set(tx);
    spawn_hook_thread()
}

fn spawn_hook_thread() -> bool {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
    std::thread::spawn(move || {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG};

        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };
        match hook {
            Ok(h) => {
                HOOK.store(h.0 as isize, Ordering::SeqCst);
                HOOK_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
                let _ = ready_tx.send(true);
                crate::logln!("keyboard hook installed for Alt+Tab");
            }
            Err(e) => {
                let _ = ready_tx.send(false);
                crate::logln!("keyboard hook NOT installed ({e}); Windows keeps Alt+Tab");
                return;
            }
        }

        // Pump until the process ends, or until a replacement hook retires this
        // thread. Nothing else runs here, so the hook is always serviced at once.
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
    });

    ready_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap_or(false)
}

/// Swap in a fresh hook. Windows removes a low-level hook without telling anyone
/// once it has been slow to answer, and only installing a new one brings it back.
fn reinstall() {
    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

    uninstall();
    let old = HOOK_THREAD.swap(0, Ordering::SeqCst);
    if old != 0 {
        unsafe {
            let _ = PostThreadMessageW(old, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
    let ok = spawn_hook_thread();
    crate::logln!("keyboard hook reinstalled -> {ok}");
}

pub fn now_ms() -> u64 {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// Whether anything other than the Win keys is held: a key, or a mouse button.
/// Either makes this press a combination, which Windows never opens Start for.
fn other_input_held() -> bool {
    let fresh_inject = now_ms().saturating_sub(LAST_INJECT_MS.load(Ordering::Relaxed)) < 120;
    (0x01..=0xFEi32).any(|vk| {
        // The Win keys themselves, and our own Ctrl tap just after injecting it.
        if vk == 0x5B || vk == 0x5C {
            return false;
        }
        if fresh_inject && matches!(vk, 0x11 | 0xA2 | 0xA3) {
            return false;
        }
        held(vk)
    })
}

/// Whether Windows' own Start - or the search pane a bare Win press opens on some
/// builds - is the window in front.
fn windows_start_in_front() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};

    let Some(h) = crate::win::apps::foreground_window() else {
        return false;
    };
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) }.max(0) as usize;
    if String::from_utf16_lossy(&buf[..n]) != "Windows.UI.Core.CoreWindow" {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    crate::win::apps::exe_path_for_pid(pid)
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase()))
        .is_some_and(|f| f == "startmenuexperiencehost.exe" || f == "searchhost.exe")
}

fn press_escape() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VK_ESCAPE,
    };
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_ESCAPE,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_BY_US,
            },
        },
    };
    unsafe {
        SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Watch the Win key from outside the hook, and cover every press the hook missed.
///
/// Reads key state only (`GetAsyncKeyState`), on its own thread, so it sits in
/// nobody's input path. It polls every 5ms while Win is down and 10ms otherwise;
/// a deliberate tap is held for 50ms or more, so no press slips between samples.
pub fn start_guard() {
    use std::time::{Duration, Instant};

    std::thread::spawn(|| {
        let mut was_down = false;
        // This press was not seen by the hook, so it is the guard's to handle.
        let mut blind = false;
        let mut seq_at_press = 0u64;
        let mut used = false;
        let mut was_open_at_press = false;
        // A bare press just ended: if Windows Start opens now, close it. Carries
        // whether that press meant "open the launcher".
        let mut watch_start: Option<(Instant, bool)> = None;
        let mut last_reinstall: Option<Instant> = None;

        loop {
            let enabled = LAUNCHER_ENABLED.load(Ordering::Relaxed);
            let down = held(0x5B) || held(0x5C);

            if down && !was_down {
                seq_at_press = PRESS_SEQ.load(Ordering::Relaxed);
                // The async key state only changes after the hook has returned, so
                // if the hook saw this press it has already said so.
                blind = !WIN_DOWN.load(Ordering::Relaxed);
                used = false;
                was_open_at_press = LAUNCHER_OPEN.load(Ordering::SeqCst);
                if blind && enabled {
                    mask_held_win();
                }
            }
            if down && !used && other_input_held() {
                used = true;
            }
            if !down && was_down {
                let hook_saw_it = PRESS_SEQ.load(Ordering::Relaxed) != seq_at_press;
                if blind && enabled && !used && !hook_saw_it {
                    crate::logln!("win guard: the hook missed a Win press; handled here");
                    send(Key::Launcher);
                }
                if !used && enabled {
                    watch_start = Some((Instant::now(), !was_open_at_press));
                }
                // A press the hook missed while someone else's window had focus
                // means the hook is gone, not merely blind to our own windows.
                let ours = crate::win::apps::foreground_window()
                    .is_some_and(|h| crate::win::apps::is_our_window(h.0 as isize));
                let due = last_reinstall.map_or(true, |t| t.elapsed() > Duration::from_secs(2));
                if blind && !hook_saw_it && !ours && due {
                    last_reinstall = Some(Instant::now());
                    reinstall();
                }
                // The key is up; a release the hook never saw must not leave it
                // thinking Win is still held.
                WIN_DOWN.store(false, Ordering::Relaxed);
                blind = false;
            }
            was_down = down;

            if let Some((at, wanted_open)) = watch_start {
                if at.elapsed() > Duration::from_millis(900) {
                    watch_start = None;
                } else if windows_start_in_front() {
                    watch_start = None;
                    crate::logln!("win guard: Windows Start opened anyway; closing it");
                    press_escape();
                    if wanted_open {
                        send(Key::OpenLauncher);
                    }
                }
            }

            let busy = down || watch_start.is_some();
            std::thread::sleep(Duration::from_millis(if busy { 5 } else { 10 }));
        }
    });
}

/// Take the hook back out. Idempotent; also happens automatically on process exit.
pub fn uninstall() {
    let raw = HOOK.swap(0, Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
        }
        crate::logln!("keyboard hook removed");
    }
}
