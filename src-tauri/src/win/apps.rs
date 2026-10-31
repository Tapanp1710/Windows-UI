//! Enumerating the windows that deserve a dock button, and acting on them.
//!
//! The selection rules below are deliberately the ones the Windows shell itself uses,
//! because the core rule of this project is that the dock must show exactly what the
//! taskbar shows. Getting this wrong in either direction is very visible: too loose
//! and invisible helper windows appear as ghost buttons, too strict and real apps go
//! missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, MAX_PATH, RECT, WPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    PostMessageW, SendMessageTimeoutW, SetForegroundWindow, ShowWindow, GA_ROOTOWNER,
    GWL_EXSTYLE, GWL_STYLE, SMTO_ABORTIFHUNG, SW_MINIMIZE, SW_RESTORE, WM_CLOSE,
    WS_CHILD, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// One top-level window that would have a taskbar button.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub pid: u32,
    pub exe_path: Option<PathBuf>,
    pub minimized: bool,
}

/// Windows belonging to one app, grouped the way the taskbar groups them.
#[derive(Debug, Clone)]
pub struct AppGroup {
    /// Stable identity: the lowercased exe path, or the pid when that is unknown.
    pub key: String,
    pub name: String,
    pub exe_path: Option<PathBuf>,
    pub windows: Vec<WindowInfo>,
}

/// Does this window get a taskbar button?
///
/// Mirrors the shell's rules: visible, not a child, not a tool window, and the root
/// owner of itself (so modal dialogs fold into their parent rather than appearing
/// separately). `WS_EX_APPWINDOW` is the documented override an app uses to force a
/// button, so it wins over the tool-window exclusion.
unsafe fn is_taskbar_window(hwnd: HWND) -> bool {
    if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
        return false;
    }

    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    if style & WS_CHILD.0 != 0 {
        return false;
    }

    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_APPWINDOW.0 == 0 {
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        // Only the root owner gets a button; owned dialogs do not get their own.
        if GetAncestor(hwnd, GA_ROOTOWNER) != hwnd {
            return false;
        }
    }

    // Windows that never activate are chrome, not apps.
    if ex & WS_EX_NOACTIVATE.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
        return false;
    }

    // Zero-area windows are placeholders (several apps keep one around).
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_ok() && (r.right - r.left) <= 0 && (r.bottom - r.top) <= 0 {
        return false;
    }

    // A title-less window is almost always a helper surface.
    GetWindowTextLengthW(hwnd) > 0
}

unsafe fn window_title(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, &mut buf);
    String::from_utf16_lossy(&buf[..n as usize])
}

/// Full path of the executable backing a process, if we are allowed to ask.
pub fn exe_path_for_pid(pid: u32) -> Option<PathBuf> {
    unsafe {
        // LIMITED_INFORMATION is the right level here: it works for processes at the
        // same integrity level without needing any elevation, which matters because
        // this app deliberately never runs as administrator.
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let mut buf = vec![0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        if ok.is_err() || len == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
    if is_taskbar_window(hwnd) {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        out.push(WindowInfo {
            hwnd: hwnd.0 as isize,
            title: window_title(hwnd),
            pid,
            exe_path: exe_path_for_pid(pid),
            minimized: IsIconic(hwnd).as_bool(),
        });
    }
    BOOL(1)
}

/// Every window that would appear on the taskbar, in Z-order (topmost first), which
/// is the order `EnumWindows` already gives us.
pub fn list_windows() -> Vec<WindowInfo> {
    let mut out: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// Group windows by application, the way the taskbar does.
pub fn list_app_groups() -> Vec<AppGroup> {
    let mut map: BTreeMap<String, AppGroup> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for w in list_windows() {
        let key = w
            .exe_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| format!("pid:{}", w.pid));

        let name = w
            .exe_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| w.title.clone());

        if !map.contains_key(&key) {
            order.push(key.clone());
            map.insert(
                key.clone(),
                AppGroup {
                    key: key.clone(),
                    name,
                    exe_path: w.exe_path.clone(),
                    windows: Vec::new(),
                },
            );
        }
        map.get_mut(&key).unwrap().windows.push(w);
    }

    order.into_iter().filter_map(|k| map.remove(&k)).collect()
}

/// The window the user is currently working in.
pub fn foreground_window() -> Option<HWND> {
    unsafe {
        let h = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        (!h.is_invalid()).then_some(h)
    }
}

/// Bring a window to the front, restoring it first if it was minimised.
///
/// `SetForegroundWindow` is subject to Windows' foreground lock: a process that does
/// not own the current foreground window is usually refused. The dock is clicked by
/// the user, so Windows normally grants it, and the `AttachThreadInput` trick that
/// some launchers use to force it is deliberately avoided here - it is exactly the
/// kind of behaviour change this project promised not to introduce.
pub fn activate(hwnd_raw: isize) {
    let hwnd = HWND(hwnd_raw as *mut _);
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Minimise a window — the second half of the taskbar's click-again behaviour.
pub fn minimize(hwnd_raw: isize) {
    let hwnd = HWND(hwnd_raw as *mut _);
    unsafe {
        if IsWindow(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
    }
}

/// Ask a window to close, exactly as the taskbar's "Close window" does: a polite
/// `WM_CLOSE` that the app may refuse (unsaved-changes prompts still appear).
pub fn close_window(hwnd_raw: isize) {
    let hwnd = HWND(hwnd_raw as *mut _);
    unsafe {
        if IsWindow(hwnd).as_bool() {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Is a window hung? Used to avoid blocking the dock on a frozen app.
#[allow(dead_code)]
pub fn is_responsive(hwnd_raw: isize) -> bool {
    let hwnd = HWND(hwnd_raw as *mut _);
    unsafe {
        let mut result = 0usize;
        SendMessageTimeoutW(
            hwnd,
            0, /* WM_NULL */
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            100,
            Some(&mut result),
        )
        .0 != 0
    }
}

/// Whether a window belongs to this process.
///
/// Used to decide what counts as "the window the user was working in": our own
/// panels never do.
pub fn is_our_window(hwnd_raw: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(HWND(hwnd_raw as *mut _), Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}
