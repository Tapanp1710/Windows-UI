//! Registering the top bar as a Windows **application desktop toolbar** (appbar).
//!
//! The first attempt at reserving screen space used `SystemParametersInfo(
//! SPI_SETWORKAREA)`. That silently did nothing: the shell recomputes the work area
//! from its registered appbars whenever anything changes, so a direct write is
//! overwritten almost immediately.
//!
//! `SHAppBarMessage` is the documented mechanism, and it is what the real taskbar
//! itself uses. Registering here means maximised windows stop below our top bar
//! exactly as they stop below the taskbar — which is the behaviour the brief asks for,
//! rather than an approximation of it.
//!
//! Registration MUST be undone (`ABM_REMOVE`) or the reserved strip outlives the app,
//! so it is wired into the same restore path as everything else.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};

/// Private message the shell uses to notify an appbar. We do not need to act on it,
/// but a valid value must be supplied at registration.
const APPBAR_CALLBACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 0x40;

static REGISTERED: AtomicBool = AtomicBool::new(false);
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: APPBAR_CALLBACK,
        ..Default::default()
    }
}

/// Reserve `height_px` physical pixels along the top of the primary monitor.
///
/// Safe to call repeatedly; later calls just move the existing appbar.
pub fn reserve_top(hwnd_raw: isize, height_px: i32) -> Option<(i32, i32, i32, i32)> {
    let hwnd = HWND(hwnd_raw as *mut _);
    let (screen_w, _) = super::taskbar::screen_size();

    unsafe {
        let mut d = data(hwnd);

        if !REGISTERED.swap(true, Ordering::SeqCst) {
            if SHAppBarMessage(ABM_NEW, &mut d) == 0 {
                REGISTERED.store(false, Ordering::SeqCst);
                crate::logln!("appbar: ABM_NEW failed; maximised windows will overlap the top bar");
                return None;
            }
            HWND_RAW.store(hwnd_raw, Ordering::SeqCst);
        }

        d.uEdge = ABE_TOP;
        d.rc = RECT {
            left: 0,
            top: 0,
            right: screen_w,
            bottom: height_px,
        };

        // The shell may move the bar (e.g. if the taskbar already owns that edge);
        // QUERYPOS tells us where it is willing to put us, and SETPOS commits it.
        SHAppBarMessage(ABM_QUERYPOS, &mut d);
        d.rc.bottom = d.rc.top + height_px;
        SHAppBarMessage(ABM_SETPOS, &mut d);

        crate::logln!(
            "appbar: reserved top strip {}..{} ({}px)",
            d.rc.top,
            d.rc.bottom,
            height_px
        );

        // The shell grants a rectangle and expects the appbar to move itself there.
        // Skipping this left the top bar sitting *below* its own reserved strip.
        Some((d.rc.left, d.rc.top, d.rc.right - d.rc.left, d.rc.bottom - d.rc.top))
    }
}

/// Give the reserved strip back. Idempotent, and safe to call when never registered.
pub fn release() {
    if !REGISTERED.swap(false, Ordering::SeqCst) {
        return;
    }
    let hwnd_raw = HWND_RAW.swap(0, Ordering::SeqCst);
    let hwnd = HWND(hwnd_raw as *mut _);
    unsafe {
        let mut d = data(hwnd);
        SHAppBarMessage(ABM_REMOVE, &mut d);
    }
    crate::logln!("appbar: released the reserved top strip");
}

/// Whether we currently hold a reservation, for the restore path to check.
pub fn is_registered() -> bool {
    REGISTERED.load(Ordering::SeqCst)
}
