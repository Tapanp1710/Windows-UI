//! The launcher: the Start menu's replacement.
//!
//! Opened by the Win key (see win/hotkeys.rs for why only its *release* is touched)
//! and by the dock's Start button. Closed by the Win key again, by Escape, or by
//! clicking away.
//!
//! The window is created hidden at startup and shown on demand. Enumerating a few
//! hundred installed apps takes long enough to notice, so it happens once, in the
//! background, rather than every time the Win key is pressed.

use std::sync::atomic::Ordering;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::win;

/// The app list, built once. `None` until the first build finishes.
static APPS: Mutex<Option<Vec<win::applist::App>>> = Mutex::new(None);

/// Build the app list on a background thread, so startup is not held up by it.
pub fn warm_up() {
    std::thread::spawn(|| {
        let apps = win::applist::all();
        crate::logln!("launcher: {} installed apps", apps.len());
        *APPS.lock().unwrap() = Some(apps);
    });
}

pub fn apps() -> Vec<win::applist::App> {
    APPS.lock().unwrap().clone().unwrap_or_default()
}

/// Toggle, which is what both the Win key and the Start button mean.
pub fn toggle(app: &AppHandle) {
    if win::hotkeys::LAUNCHER_OPEN.load(Ordering::SeqCst) {
        close(app);
    } else {
        open(app);
    }
}

pub fn open(app: &AppHandle) {
    let Some(w) = app.get_webview_window("launcher") else {
        // No window means the launcher could not be built. Rather than do nothing,
        // fall back to the real Start menu - the brief's own instruction for when
        // the launcher cannot stand in for it.
        crate::logln!("launcher window missing; opening Windows Start instead");
        win::press_ctrl_esc();
        return;
    };
    win::hotkeys::LAUNCHER_OPEN.store(true, Ordering::SeqCst);
    let _ = app.emit("launcher:open", ());
    let _ = w.show();
    let _ = w.set_focus();
}

pub fn close(app: &AppHandle) {
    if !win::hotkeys::LAUNCHER_OPEN.swap(false, Ordering::SeqCst) {
        return;
    }
    let _ = app.emit("launcher:close", ());
    if let Some(w) = app.get_webview_window("launcher") {
        let _ = w.hide();
    }
}

/// Sleep, restart, shut down, sign out — the same four Start offers.
///
/// Restart and shut down go through `shutdown.exe` rather than `ExitWindowsEx`,
/// because that call needs `SE_SHUTDOWN_NAME` enabled on our token first and
/// `shutdown.exe` already does all of that correctly.
pub fn power(action: &str) -> bool {
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    crate::logln!("power action requested: {action}");
    let spawned = match action {
        "sleep" => Command::new("rundll32.exe")
            .args(["powrprof.dll,SetSuspendState", "0,1,0"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn(),
        "restart" => Command::new("shutdown.exe")
            .args(["/r", "/t", "0"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn(),
        "shutdown" => Command::new("shutdown.exe")
            .args(["/s", "/t", "0"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn(),
        "signout" => Command::new("shutdown.exe")
            .args(["/l"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn(),
        _ => return false,
    };

    match spawned {
        Ok(_) => true,
        Err(e) => {
            crate::logln!("power action {action} failed: {e}");
            false
        }
    }
}
