//! Live window previews, through the DWM thumbnail API.
//!
//! `DwmRegisterThumbnail` asks the compositor to draw another window's current
//! contents inside ours. It is the same mechanism Windows' own Alt+Tab and taskbar
//! previews use, and it is genuinely live - no screenshotting, no polling, no cost to
//! us per frame.
//!
//! One thing shapes the whole design around it: the thumbnail is composited **over**
//! our window by DWM, not drawn into the page. Nothing the webview paints at those
//! coordinates will be visible, and no CSS transition reaches it. So the page leaves
//! a hole where the preview goes, and every fade happens here, through the
//! thumbnail's own opacity.
//!
//! The switcher shows one preview at a time. Changing it cross-fades: the new
//! window is registered invisible and eased in while the old ones ease out and are
//! released once they reach zero.
//!
//! Registrations must be released. A leaked thumbnail keeps a reference to a window
//! that may be closing, so every path out of the switcher goes through `clear`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
    DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};

struct Thumb {
    handle: isize,
    source: isize,
    alpha: f32,
}

/// Everything registered. The last one is the preview being shown; any before it
/// are on their way out.
static LIVE: Mutex<Vec<Thumb>> = Mutex::new(Vec::new());
/// Whether the fade thread is running.
static ANIMATING: AtomicBool = AtomicBool::new(false);

/// One fade step. Each step closes this share of the remaining distance, which is
/// an ease-out: quick to start, soft to land, ~150ms to settle.
const STEP: Duration = Duration::from_millis(14);
const EASE: f32 = 0.28;

/// Show `source` inside `dest`, fitted into a rectangle in `dest`'s client
/// coordinates (physical pixels) with its own aspect ratio, and cross-fade to it
/// from whatever was showing. Showing the window already in front does nothing.
pub fn present(dest: isize, source: isize, x: i32, y: i32, w: i32, h: i32) -> bool {
    if w <= 0 || h <= 0 {
        return false;
    }
    if LIVE.lock().unwrap().last().is_some_and(|t| t.source == source) {
        return true;
    }
    unsafe {
        let Ok(thumb) = DwmRegisterThumbnail(HWND(dest as *mut _), HWND(source as *mut _)) else {
            return false;
        };

        // Letterboxed, not stretched: a tall window squashed into a wide hole reads
        // as a different window.
        let (mut dx, mut dy, mut dw, mut dh) = (x, y, w, h);
        if let Ok(size) = DwmQueryThumbnailSourceSize(thumb) {
            if size.cx > 0 && size.cy > 0 {
                let k = (w as f32 / size.cx as f32).min(h as f32 / size.cy as f32);
                dw = (size.cx as f32 * k).round() as i32;
                dh = (size.cy as f32 * k).round() as i32;
                dx = x + (w - dw) / 2;
                dy = y + (h - dh) / 2;
            }
        }

        let animate = crate::win::animations_enabled();
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_VISIBLE
                | DWM_TNP_OPACITY
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: RECT {
                left: dx,
                top: dy,
                right: dx + dw,
                bottom: dy + dh,
            },
            opacity: if animate { 0 } else { 255 },
            fVisible: true.into(),
            fSourceClientAreaOnly: true.into(),
            ..Default::default()
        };

        if DwmUpdateThumbnailProperties(thumb, &props).is_err() {
            let _ = DwmUnregisterThumbnail(thumb);
            return false;
        }

        let mut live = LIVE.lock().unwrap();
        if !animate {
            // No fade: the old ones go at once.
            for t in live.drain(..) {
                let _ = DwmUnregisterThumbnail(t.handle);
            }
        }
        live.push(Thumb {
            handle: thumb,
            source,
            alpha: if animate { 0.0 } else { 255.0 },
        });
    }
    run_fades();
    true
}

fn set_alpha(handle: isize, alpha: f32) {
    let props = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_OPACITY,
        opacity: alpha.round().clamp(0.0, 255.0) as u8,
        ..Default::default()
    };
    unsafe {
        let _ = DwmUpdateThumbnailProperties(handle, &props);
    }
}

/// Ease every preview toward its target - the newest to fully visible, the rest to
/// gone - on one thread that stops as soon as everything has arrived.
fn run_fades() {
    if ANIMATING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(STEP);
        let mut live = LIVE.lock().unwrap();
        let n = live.len();
        let mut settled = true;
        for (i, t) in live.iter_mut().enumerate() {
            let target = if i + 1 == n { 255.0 } else { 0.0 };
            let next = t.alpha + (target - t.alpha) * EASE;
            t.alpha = if (target - next).abs() < 4.0 { target } else { next };
            if t.alpha != target {
                settled = false;
            }
            set_alpha(t.handle, t.alpha);
        }
        // Faded-out ones are released straight away.
        live.retain(|t| {
            let gone = t.alpha == 0.0;
            if gone {
                unsafe {
                    let _ = DwmUnregisterThumbnail(t.handle);
                }
            }
            !gone
        });
        if settled {
            // Cleared under the same lock `present` pushes under, so a preview
            // added right now either is seen above or starts a new thread.
            ANIMATING.store(false, Ordering::SeqCst);
            return;
        }
    });
}

/// Release every registration. Idempotent, and safe to call when there are none.
pub fn clear() {
    let taken: Vec<Thumb> = std::mem::take(&mut *LIVE.lock().unwrap());
    for t in taken {
        unsafe {
            let _ = DwmUnregisterThumbnail(t.handle);
        }
    }
}
