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

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
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
}

static TX: OnceLock<Sender<Key>> = OnceLock::new();
static HOOK: AtomicIsize = AtomicIsize::new(0);

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

    let events = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
        key(VIRTUAL_KEY(win_vk), KEYEVENTF_KEYUP),
    ];
    SendInput(&events, std::mem::size_of::<INPUT>() as i32);
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
            if up && (vk == VK_LMENU.0 || vk == VK_RMENU.0) {
                // Deliberately NOT swallowed - see the module comment.
                send(Key::Commit);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
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

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG};

        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };
        match hook {
            Ok(h) => {
                HOOK.store(h.0 as isize, Ordering::SeqCst);
                let _ = ready_tx.send(true);
                crate::logln!("keyboard hook installed for Alt+Tab");
            }
            Err(e) => {
                let _ = ready_tx.send(false);
                crate::logln!("keyboard hook NOT installed ({e}); Windows keeps Alt+Tab");
                return;
            }
        }

        // Pump until the process ends. Nothing else runs on this thread, so the
        // hook is always serviced immediately.
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
