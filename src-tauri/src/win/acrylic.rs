//! Real blur behind a window, so "glass" actually means glass.
//!
//! `backdrop-filter` in a webview can only blur what the PAGE draws behind an
//! element, which on a transparent window is nothing at all (D-009). The desktop
//! behind the window is invisible to CSS. The only way to blur it is to ask the
//! compositor, and the way Windows exposes that is `SetWindowCompositionAttribute`.
//!
//! That function is undocumented: it is exported from `user32.dll` but has no header
//! and no import library, so it is looked up by name at runtime. If the lookup fails,
//! or the call fails, nothing happens and the surfaces fall back to the painted
//! translucency they had before - which is why every caller ignores the result.
//!
//! This changes nothing about the system: the attribute lives on our own window
//! handle and dies with it.

use std::ffi::c_void;

use windows::core::s;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

#[repr(C)]
#[derive(Clone, Copy)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    /// ABGR, not ARGB. The alpha is how much of this tint is laid over the blur.
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

/// ACCENT_ENABLE_ACRYLICBLURBEHIND — the Windows 10 1803+ acrylic material.
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
/// ACCENT_DISABLED
const ACCENT_DISABLED: u32 = 0;
/// WCA_ACCENT_POLICY
const WCA_ACCENT_POLICY: u32 = 19;

type SetWindowCompositionAttributeFn =
    unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> BOOL;

fn entry_point() -> Option<SetWindowCompositionAttributeFn> {
    unsafe {
        let user32 = GetModuleHandleA(s!("user32.dll")).ok()?;
        let proc = GetProcAddress(user32, s!("SetWindowCompositionAttribute"))?;
        Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            SetWindowCompositionAttributeFn,
        >(proc))
    }
}

fn apply(hwnd_raw: isize, state: u32, tint: u32) -> bool {
    let Some(f) = entry_point() else {
        return false;
    };
    let mut policy = AccentPolicy {
        accent_state: state,
        // 0, not 2. Flag 2 asks the composition layer to draw its own border around
        // the acrylic, and that border is a bright near-white line regardless of the
        // tint - the white ring that showed around every panel. The hairline we do
        // want comes from DWMWA_BORDER_COLOR in `round_corners`, which follows the
        // theme. The blur still covers the whole window without this flag.
        accent_flags: 0,
        gradient_color: tint,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy as *mut _ as *mut c_void,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    unsafe { f(HWND(hwnd_raw as *mut _), &mut data).as_bool() }
}

/// Turn a window into blurred glass.
///
/// `tint` is **ABGR**: the colour laid over the blur, and its alpha is how dense that
/// tint is. Too little and text on the panel is unreadable over a busy wallpaper; too
/// much and there was no point blurring. Around 60% reads as glass and stays legible.
pub fn enable(hwnd_raw: isize, tint_abgr: u32) -> bool {
    let ok = apply(hwnd_raw, ACCENT_ENABLE_ACRYLICBLURBEHIND, tint_abgr);
    crate::logln!("acrylic: enable on {hwnd_raw:#x} -> {ok}");
    ok
}

/// Give the glass back, e.g. if the user turns the effect off.
pub fn disable(hwnd_raw: isize) {
    apply(hwnd_raw, ACCENT_DISABLED, 0);
}

/// The tint for the current theme, as the ABGR value `enable` wants.
///
/// Warm, matching the project's palette: a light cream over the blur in light mode,
/// a warm near-black in dark mode.
pub fn tint_for(dark: bool) -> u32 {
    if dark {
        // 0xAABBGGRR — alpha 0xA8 over #0D1830, the dark end of the palette.
        0xA8_30_18_0D
    } else {
        // alpha 0xB4 over #F7FAFE, the light end.
        0xB4_FE_FA_F7
    }
}

/// Round a window's corners and give it a hairline border, at the DWM level.
///
/// **Not** `SetWindowRgn`. The acrylic backdrop is composited by DWM against the
/// window's *rectangle* and ignores the window region entirely - measured: a region
/// with 21px corners produced a panel with perfectly square ones. `DWMWA_WINDOW_
/// CORNER_PREFERENCE` is the Windows 11 attribute that DWM itself honours, so the
/// glass is clipped to the rounded shape and the border follows the curve.
///
/// Silently does nothing before Windows 11, where the attributes do not exist; the
/// panel is then a square one, which is what that Windows looks like anyway.
pub fn round_corners(hwnd_raw: isize, border_bgr: u32) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };

    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const c_void,
            std::mem::size_of_val(&pref) as u32,
        );
        let colour = border_bgr;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &colour as *const _ as *const c_void,
            std::mem::size_of_val(&colour) as u32,
        );
    }
}

/// Border colour for the theme.
///
/// **`DWMWA_COLOR_NONE` in both themes.** Every surface here is a floating island on
/// a transparent window; a system-drawn hairline around it reads as a ring, and in
/// the light theme that ring is near-white, which is what kept showing up around
/// every pop-up. The surfaces carry their own edge in CSS, where it follows the
/// palette. Kept as a function taking `dark` so a theme can ask for a border again
/// without changing every call site. See D-042 and D-045.
pub fn border_for(_dark: bool) -> u32 {
    DWMWA_COLOR_NONE
}

/// DWM's "draw no border at all" sentinel.
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
