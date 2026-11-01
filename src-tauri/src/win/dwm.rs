//! Live window previews, through the DWM thumbnail API.
//!
//! `DwmRegisterThumbnail` asks the compositor to draw another window's current
//! contents inside ours. It is the same mechanism Windows' own Alt+Tab and taskbar
//! previews use, and it is genuinely live - no screenshotting, no polling, no cost to
//! us per frame.
//!
//! One thing shapes the whole design around it: the thumbnail is composited **over**
//! our window by DWM, not drawn into the page. Nothing the webview paints at those
//! coordinates will be visible. So the page leaves holes where the previews go, and
//! hands their rectangles back for registration.
//!
//! Registrations must be released. A leaked thumbnail keeps a reference to a window
//! that may be closing, so every path out of the switcher goes through `clear`.

use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};

/// Everything currently registered, so it can all be released at once.
static LIVE: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Show `source` inside `dest` at a rectangle in `dest`'s client coordinates.
///
/// `client_only` trims the source's frame and shadow, which is what makes a row of
/// previews line up rather than each one carrying its own invisible border.
///
/// `opacity` lets the switcher register its previews invisible and fade them in with
/// its own bar: DWM draws them over the page, so a CSS fade never reaches them.
pub fn show(dest: isize, source: isize, x: i32, y: i32, w: i32, h: i32, opacity: u8) -> bool {
    if w <= 0 || h <= 0 {
        return false;
    }
    unsafe {
        let Ok(thumb) = DwmRegisterThumbnail(HWND(dest as *mut _), HWND(source as *mut _)) else {
            return false;
        };

        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_VISIBLE
                | DWM_TNP_OPACITY
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            opacity,
            fVisible: true.into(),
            fSourceClientAreaOnly: true.into(),
            ..Default::default()
        };

        if DwmUpdateThumbnailProperties(thumb, &props).is_err() {
            let _ = DwmUnregisterThumbnail(thumb);
            return false;
        }
        LIVE.lock().unwrap().push(thumb);
        true
    }
}

/// Release every registration. Idempotent, and safe to call when there are none.
pub fn clear() {
    let taken: Vec<isize> = std::mem::take(&mut *LIVE.lock().unwrap());
    for thumb in taken {
        unsafe {
            let _ = DwmUnregisterThumbnail(thumb);
        }
    }
}

/// Set the opacity of every live registration, for fading them in.
pub fn set_opacity(opacity: u8) {
    let live = LIVE.lock().unwrap().clone();
    let props = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_OPACITY,
        opacity,
        ..Default::default()
    };
    for thumb in live {
        unsafe {
            let _ = DwmUpdateThumbnailProperties(thumb, &props);
        }
    }
}
