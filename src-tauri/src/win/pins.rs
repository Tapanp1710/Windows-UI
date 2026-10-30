//! Importing the user's existing Windows taskbar pins, and launching things.
//!
//! Windows keeps taskbar pins as ordinary shortcut files in
//! `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`.
//! Reading that folder is how we honour "import my current Windows taskbar pins on
//! first run" without touching the registry or the shell's own state.
//!
//! The pin ORDER lives in a binary registry blob (`Taskband\FavoritesResolve`) whose
//! format is undocumented and version-specific. Rather than guess at it, the import
//! sorts alphabetically and the user can drag to reorder — which the dock persists
//! itself. That trade-off is recorded in DECISIONS.md (D-004).

use std::path::{Path, PathBuf};

use crate::win::icons;

#[derive(Debug, Clone)]
pub struct Pin {
    /// Stable id for the settings file. Derived from the shortcut filename.
    pub id: String,
    pub name: String,
    /// What to hand to the shell to launch or to draw an icon for.
    pub parsing_name: String,
}

/// Folder holding the user's taskbar pins.
pub fn taskbar_pins_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(appdata)
        .join("Microsoft")
        .join("Internet Explorer")
        .join("Quick Launch")
        .join("User Pinned")
        .join("TaskBar");
    dir.is_dir().then_some(dir)
}

/// Read the current Windows taskbar pins, alphabetically.
pub fn read_taskbar_pins() -> Vec<Pin> {
    let Some(dir) = taskbar_pins_dir() else {
        return Vec::new();
    };

    let mut pins: Vec<Pin> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return pins;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_link = path
            .extension()
            .map(|e| {
                let e = e.to_string_lossy().to_lowercase();
                e == "lnk" || e == "url"
            })
            .unwrap_or(false);
        if !is_link {
            continue;
        }

        let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };

        pins.push(Pin {
            id: format!("pin:{}", stem.to_lowercase()),
            name: stem,
            parsing_name: path.to_string_lossy().to_string(),
        });
    }

    pins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    pins
}

/// Icon for a pin, as a PNG data URL.
pub fn pin_icon(pin: &Pin) -> Option<String> {
    icons::icon_data_url(&pin.parsing_name)
}

/// Launch something by its shell parsing name (an exe, a .lnk, or
/// `shell:AppsFolder\<AUMID>`), exactly as double-clicking it in Explorer would.
///
/// Uses `ShellExecuteW` with the default verb rather than `CreateProcess`, so
/// shortcuts, working directories, arguments, UWP activation and per-file handlers
/// all behave the way Windows intends.
pub fn launch(parsing_name: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = HSTRING::from(parsing_name);
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::PCWSTR(file.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a fake HINSTANCE; anything > 32 means success.
    result.0 as isize > 32
}

/// Open a folder in File Explorer, used by the dock's and launcher's context menus.
pub fn reveal_in_explorer(path: &Path) -> bool {
    launch(&path.to_string_lossy())
}

/// The Recycle Bin, which the shell addresses by a well-known parsing name rather
/// than a filesystem path.
pub const RECYCLE_BIN: &str = "shell:RecycleBinFolder";

/// Whether the Recycle Bin currently has anything in it, so the dock can show the
/// full vs empty icon the way Explorer does.
pub fn recycle_bin_is_empty() -> bool {
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if SHQueryRecycleBinW(windows::core::PCWSTR::null(), &mut info).is_ok() {
            info.i64NumItems == 0
        } else {
            true
        }
    }
}

/// Resolve a `.lnk` shortcut to the executable it points at.
///
/// Needed so a pinned shortcut can be matched against a *running* process: the dock
/// has to know that the "Firefox" pin and the running `firefox.exe` are the same app,
/// otherwise a pinned app would appear twice the moment you launched it.
///
/// `SLR_NO_UI | SLR_NOSEARCH` keeps this quiet and fast: a broken shortcut returns
/// `None` instead of popping up Windows' "item has been moved" repair dialog.
pub fn resolve_lnk_target(lnk_path: &Path) -> Option<PathBuf> {
    use windows::core::{Interface, HSTRING, PWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH, SLR_NOSEARCH, SLR_NO_UI};
    use windows::Win32::Foundation::MAX_PATH;

    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let path = HSTRING::from(lnk_path.as_os_str());
        persist.Load(windows::core::PCWSTR(path.as_ptr()), STGM_READ).ok()?;

        // Best-effort resolve; a missing target is not worth a dialog.
        let _ = link.Resolve(None, (SLR_NO_UI.0 | SLR_NOSEARCH.0) as u32);

        let mut buf = vec![0u16; MAX_PATH as usize];
        link.GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32).ok()?;
        let _ = PWSTR::null();

        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if end == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..end])))
    }
}
