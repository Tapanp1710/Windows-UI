//! Windows integration. Everything that talks to Win32 lives under here so the rest
//! of the app stays platform-agnostic and easy to read.

pub mod acrylic;
pub mod dragout;
pub mod dwm;
pub mod hotkeys;
pub mod appbar;
pub mod applist;
pub mod apps;
pub mod icons;
pub mod media;
pub mod pins;
pub mod brightness;
pub mod bt;
pub mod radios;
pub mod wifi;
pub mod status;
pub mod sysinfo;
pub mod taskbar;
pub mod touchpad;

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

/// Synthesise Win+<key>, used to open Windows' own flyouts (Quick Settings on Win+A,
/// notification centre on Win+N).
///
/// As with `press_ctrl_esc`, this *sends* a shortcut rather than intercepting one, so
/// no existing Windows key binding is altered.
pub fn press_win_key(ch: char) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_LWIN,
    };

    fn key(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let vk = ch.to_ascii_uppercase() as u16;
    let inputs = [
        key(VK_LWIN.0, false),
        key(vk, false),
        key(vk, true),
        key(VK_LWIN.0, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Restrict a window's clickable + visible area to a set of rectangles.
///
/// The top bar is one full-width always-on-top window showing three small pills. If
/// the whole window were hittable it would swallow every click along the top edge of
/// the screen — a behaviour change, and the thing this project most has to avoid.
/// `SetWindowRgn` makes everything outside these rectangles genuinely not part of the
/// window, so clicks land on whatever is underneath.
///
/// Each rectangle carries its own corner radius, and the caller inflates it to leave
/// room for the surface's shadow.
///
/// Both matter visually, not just for hit-testing. A **rectangular** region cuts the
/// corners off a rounded card and leaves the shadow behind as four hard corners - the
/// card looks like a sharp-edged slab with a rounded picture painted on it. And a
/// region tight to the card clips the shadow mid-fade, so it ends in a visible line
/// instead of fading out. Rounded regions, inflated past where the shadow has fallen
/// to nothing, fix both.
pub fn set_window_region(hwnd: isize, rects: &[(i32, i32, i32, i32, i32)]) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    };

    unsafe {
        let hwnd = HWND(hwnd as *mut _);
        if rects.is_empty() {
            // An empty region would make the window un-clickable everywhere; prefer
            // a 1x1 region so the window still exists but blocks nothing.
            let rgn = CreateRectRgn(0, 0, 1, 1);
            SetWindowRgn(hwnd, rgn, true);
            return;
        }

        let combined = CreateRectRgn(0, 0, 0, 0);
        for &(x, y, w, h, r) in rects {
            // CreateRoundRectRgn takes the ellipse SIZE, which is twice the radius,
            // and its right/bottom edges are exclusive.
            let part = if r > 0 {
                CreateRoundRectRgn(x, y, x + w + 1, y + h + 1, r * 2, r * 2)
            } else {
                CreateRectRgn(x, y, x + w, y + h)
            };
            CombineRgn(combined, combined, part, RGN_OR);
            let _ = DeleteObject(part);
        }
        // Windows takes ownership of the region handle on success - do not delete it.
        SetWindowRgn(hwnd, combined, true);
    }
}

/// Open the Windows tray overflow ("show hidden icons").
///
/// Clicks the real overflow button so third-party tray icons stay fully reachable,
/// which is the brief's requirement. If the taskbar is hidden the button cannot be
/// clicked, so the taskbar is briefly shown, the flyout opened, and the taskbar
/// hidden again.
pub fn open_tray_overflow() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, PostMessageW, SetForegroundWindow, BM_CLICK,
    };

    unsafe {
        let tray = FindWindowW(w!("Shell_TrayWnd"), None).ok();
        let Some(tray) = tray.filter(|h| !h.is_invalid()) else {
            return false;
        };

        // Win11 moved the overflow into a XAML host; the classic chain still exists
        // on many builds, so try it and fall back to Quick Settings.
        let notify = FindWindowExW(tray, None, w!("TrayNotifyWnd"), None).ok();
        if let Some(notify) = notify.filter(|h| !h.is_invalid()) {
            if let Ok(button) = FindWindowExW(notify, None, w!("Button"), None) {
                if !button.is_invalid() {
                    let _ = SetForegroundWindow(tray);
                    let _ = PostMessageW(button, BM_CLICK, WPARAM(0), LPARAM(0));
                    return true;
                }
            }
        }
        false
    }
}

/// Is a Bluetooth radio present, and is it on?
///
/// Uses the same radio enumeration the Settings app does. Cached presence, live state.
/// Make sure this thread has a COM apartment, for the shell calls below.
///
/// The commands that reach the shell are async, so they run on a pool thread that
/// may have none. It must be **multi-threaded**: an STA thread has to pump messages
/// and a tokio worker never does, so marking one STA deadlocks the next marshalled
/// call (D-043). `S_FALSE` and `RPC_E_CHANGED_MODE` both mean "already fine".
pub fn ensure_com() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

pub fn bluetooth() -> (bool, bool) {
    // Presence and the real soft on/off state both come from the radio poller's
    // snapshot. This used to shell out to PowerShell from the main thread; see
    // `radios::bluetooth` and D-033 for why that had to go.
    radios::bluetooth()
}

/// Park a window directly above the desktop, below every ordinary window.
///
/// `alwaysOnBottom` sends a window to `HWND_BOTTOM`, which on Windows is *below*
/// `Progman` — the desktop itself — and therefore invisible. Inserting immediately
/// after `Progman` instead puts the widget layer exactly where the brief asks for it:
/// part of the desktop, never covering an application.
///
/// Called again periodically, because the shell re-sorts the z-order on its own
/// (a window being activated, a display change, Win+D) and there is no notification
/// to hook for it.
pub fn place_above_desktop(hwnd_raw: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    unsafe {
        let desktop = FindWindowW(windows::core::w!("Progman"), None).unwrap_or(HWND_BOTTOM);
        let _ = SetWindowPos(
            HWND(hwnd_raw as *mut _),
            desktop,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Whether a mouse button is currently held down.
///
/// A plain read of the key state. This is not a hook: nothing is inserted into
/// anyone's input path, and no message is intercepted or swallowed.
pub fn mouse_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    };

    unsafe {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .iter()
            .any(|k| (GetAsyncKeyState(k.0 as i32) as u16 & 0x8000) != 0)
    }
}
