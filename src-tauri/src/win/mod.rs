//! Windows integration. Everything that talks to Win32 lives under here so the rest
//! of the app stays platform-agnostic and easy to read.

pub mod apps;
pub mod icons;
pub mod pins;
pub mod taskbar;

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// COM must be initialised on this thread before any shell interface is used
/// (`IShellItemImageFactory` for icons, `ShellExecuteW` for launching).
/// Apartment-threaded is what shell interfaces expect.
pub fn init_com() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    unsafe {
        // Returns S_FALSE if COM is already initialised on this thread, which is
        // fine and not an error.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// Declare per-monitor-v2 DPI awareness.
///
/// Must run before any window exists. On this windows-gnu build the embedded manifest
/// is mangled at link time (`.rsrc merge failure: multiple non-default manifests`), so
/// this is belt-and-braces: if the manifest did set awareness the call is a harmless
/// no-op, and if it did not, this fixes it.
pub fn init_dpi_awareness() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Current mouse position in physical screen pixels.
pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Whether Windows' "Animation effects" setting is on.
///
/// The brief asks us to respect it: if the user has animations off system-wide, the
/// dock should not slide and scale either.
pub fn animations_enabled() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut enabled = windows::Win32::Foundation::BOOL(1);
    unsafe {
        let ok = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        // If the query fails, assume animations are wanted rather than silently
        // shipping a static UI.
        ok.is_ok() && enabled.as_bool() || ok.is_err()
    }
}

/// Synthesise Ctrl+Esc, which asks the shell to open the Start menu.
///
/// `SendInput` is used rather than a keyboard hook: this *sends* a keystroke, it does
/// not intercept or remap one, so every existing Windows shortcut keeps working
/// untouched. That distinction is the whole point of the project's core rule.
pub fn press_ctrl_esc() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_ESCAPE,
    };

    fn key(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let inputs = [
        key(VK_CONTROL.0, false),
        key(VK_ESCAPE.0, false),
        key(VK_ESCAPE.0, true),
        key(VK_CONTROL.0, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}
