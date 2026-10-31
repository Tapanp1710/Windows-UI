//! The one keyboard hook in this project, and the rules it lives by.
//!
//! Alt+Tab cannot be observed any other way: the shell reserves it, so a
//! `WH_KEYBOARD_LL` hook is the only way to put something else on it. Everything else
//! in this project deliberately avoids hooks (see autohide.rs), so this one is kept
//! as small and as inert as possible.
//!
//! **What it swallows, and nothing else:**
//!   - `Tab` while Alt is physically held — the whole point.
//!   - `Esc` while the switcher is open — cancelling.
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
//! The callback therefore does the least work that can possibly answer "swallow or
//! not": it reads two key states and pushes an enum down a channel. It never touches
//! a window, never allocates, and never blocks.

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
}

static TX: OnceLock<Sender<Key>> = OnceLock::new();
static HOOK: AtomicIsize = AtomicIsize::new(0);

/// Whether the switcher is on screen. Read by the hook to decide whether Esc and the
/// Alt release mean anything, so it must be kept in step by the UI side.
pub static SWITCHER_OPEN: AtomicBool = AtomicBool::new(false);

/// Master switch. When false the hook passes everything through untouched, which is
/// the setting-level way to hand Alt+Tab back to Windows.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

fn held(vk: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LMENU, VK_MENU, VK_RMENU, VK_SHIFT, VK_TAB};

    // HC_ACTION is 0. Anything else must be passed on untouched.
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
