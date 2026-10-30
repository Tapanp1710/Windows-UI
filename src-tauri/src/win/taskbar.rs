//! Hiding and restoring the Windows taskbar, and managing the desktop work area.
//!
//! Everything here is deliberately conservative. The taskbar is the user's lifeline:
//! if this module leaves it hidden, the machine is close to unusable until they find
//! `restore-windows.ps1`. So every hide has a matching restore on every exit path, and
//! a separate watchdog process (see `crate::watchdog`) performs the restore again if
//! this process dies without running its own cleanup.
//!
//! None of these calls need administrator rights.

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_ALWAYSONTOP, ABS_AUTOHIDE, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetSystemMetrics, IsWindowVisible, SetWindowPos, ShowWindow,
    SystemParametersInfoW, HWND_BOTTOM, SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA,
    SPI_SETWORKAREA, SPIF_SENDCHANGE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW,
};

/// The primary taskbar window class. Stable across Windows 7..11.
fn tray_hwnd() -> Option<HWND> {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok().filter(|h| !h.is_invalid())
}

/// Taskbars on secondary monitors, if any.
fn secondary_tray_hwnd() -> Option<HWND> {
    unsafe { FindWindowW(w!("Shell_SecondaryTrayWnd"), None) }
        .ok()
        .filter(|h| !h.is_invalid())
}

/// Raw `ABS_*` bitmask describing the taskbar's auto-hide / always-on-top state.
pub fn appbar_state() -> u32 {
    unsafe {
        let mut data = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            ..Default::default()
        };
        SHAppBarMessage(ABM_GETSTATE, &mut data) as u32
    }
}

/// Set the taskbar's auto-hide / always-on-top state to a raw `ABS_*` bitmask.
pub fn set_appbar_state(state: u32) {
    unsafe {
        let mut data = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: tray_hwnd().unwrap_or_default(),
            lParam: LPARAM(state as isize),
            ..Default::default()
        };
        SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
}

/// Whether the taskbar is currently hidden by us (or anything else).
pub fn is_taskbar_visible() -> bool {
    tray_hwnd().map(|h| unsafe { IsWindowVisible(h) }.as_bool()).unwrap_or(false)
}

/// Hide the Windows taskbar.
///
/// `ShowWindow(SW_HIDE)` is used rather than only setting auto-hide, because with
/// auto-hide alone the real taskbar would still slide up whenever the pointer reaches
/// the bottom edge — which is exactly the gesture that reveals our dock, so the two
/// would fight over the same screen edge.
///
/// This is process-external state: it survives our death, which is why the watchdog
/// exists.
pub fn hide_taskbar() {
    if let Some(h) = tray_hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
    if let Some(h) = secondary_tray_hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

/// Bring the Windows taskbar back exactly as it was.
///
/// `restore_state` is the raw `ABS_*` value captured before we hid anything. Passing
/// the captured value matters: this machine already had auto-hide switched on, and
/// forcing "always visible" here would silently change a setting the user chose.
///
/// Safe to call repeatedly and safe to call when nothing was hidden.
pub fn restore_taskbar(restore_state: Option<u32>) {
    if let Some(h) = tray_hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
            // Put it back at the bottom of the Z-order band it normally occupies
            // rather than leaving it floating above everything.
            let _ = SetWindowPos(h, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    if let Some(h) = secondary_tray_hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
        }
    }
    if let Some(state) = restore_state {
        set_appbar_state(state);
    }
}

/// The current desktop work area, in physical pixels.
pub fn work_area() -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut r as *mut RECT as *mut _),
            Default::default(),
        );
    }
    r
}

/// Full primary-monitor bounds in physical pixels.
pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// Reserve `bottom_px` physical pixels at the bottom of the primary monitor so that
/// maximised windows stop above our dock.
///
/// Pass 0 to hand the whole screen back, which is what an auto-hiding dock wants:
/// Windows' own behaviour for an auto-hidden taskbar is that maximised windows use the
/// full height, and the brief asks us to match that exactly.
pub fn set_reserved_bottom(bottom_px: i32) {
    let (w, h) = screen_size();
    let mut r = RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h - bottom_px.max(0),
    };
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_SETWORKAREA,
            0,
            Some(&mut r as *mut RECT as *mut _),
            SPIF_SENDCHANGE,
        );
    }
}

/// Hand the entire primary monitor back to Windows. The shell immediately re-subtracts
/// whatever the real taskbar needs, so this lands on the correct default rather than
/// on a guess.
pub fn reset_work_area() {
    set_reserved_bottom(0);
}

/// Convenience used by both the app's own cleanup and the watchdog.
pub fn full_restore(restore_state: Option<u32>) {
    restore_taskbar(restore_state);
    reset_work_area();
}

/// Human-readable description of a raw `ABS_*` mask, for the log.
pub fn describe_state(state: u32) -> String {
    let mut parts = Vec::new();
    if state & ABS_AUTOHIDE != 0 {
        parts.push("auto-hide");
    }
    if state & ABS_ALWAYSONTOP != 0 {
        parts.push("always-on-top");
    }
    if parts.is_empty() {
        parts.push("none");
    }
    format!("{} (raw {})", parts.join("+"), state)
}
