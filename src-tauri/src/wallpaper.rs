//! Offering the project's own wallpaper, never forcing it.
//!
//! The brief asks for an original warm abstract wallpaper "offered in settings but not
//! force-applied", so nothing here runs unless the user presses a button. The path
//! Windows had before is recorded in `settings.json` first, so the change is
//! reversible from the settings window and from `restore-windows.ps1`.
//!
//! This is the one place the app changes a *persistent* Windows setting
//! (`SPIF_UPDATEINIFILE` makes Windows store it), which is why it is opt-in, recorded,
//! and listed in SYSTEM_CHANGES.md.

use std::path::PathBuf;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::MAX_PATH;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETDESKWALLPAPER,
    SPI_SETDESKWALLPAPER,
};

/// The two bundled wallpapers, keyed by the name the settings UI sends.
fn asset_name(which: &str) -> Option<&'static str> {
    match which {
        "light" => Some("macui-warm-light-1920x1080.png"),
        "dark" => Some("macui-warm-dark-1920x1080.png"),
        _ => None,
    }
}

/// Whatever Windows currently has set, as a path. Empty string means "no wallpaper",
/// which is a legitimate state and must round-trip.
pub fn current() -> Option<String> {
    let mut buf = vec![0u16; MAX_PATH as usize];
    unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            Default::default(),
        )
        .ok()?;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Ask Windows to use `path`. An empty path clears the wallpaper.
fn apply(path: &str) -> bool {
    let h = HSTRING::from(path);
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(PCWSTR(h.as_ptr()).as_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
        .is_ok()
    }
}

/// Unpack a bundled wallpaper next to the settings file so Windows has a real path to
/// point at — the image itself only exists inside the executable.
fn extract(app: &tauri::AppHandle, asset: &str) -> Option<PathBuf> {
    let dir = crate::settings::settings_path().parent()?.to_path_buf();
    std::fs::create_dir_all(&dir).ok()?;
    let out = dir.join(asset);

    if !out.is_file() {
        let resolver = app.asset_resolver();
        let found = resolver
            .get(format!("/wallpaper/{asset}"))
            .or_else(|| resolver.get(format!("wallpaper/{asset}")))?;
        std::fs::write(&out, found.bytes).ok()?;
    }
    Some(out)
}

/// `which` is "light", "dark", or "restore".
pub fn set(app: &tauri::AppHandle, which: &str) -> bool {
    if which == "restore" {
        let Some(original) = crate::settings::load().original_wallpaper else {
            crate::logln!("wallpaper: nothing recorded to restore");
            return false;
        };
        let ok = apply(&original);
        if ok {
            crate::settings::update(|s| s.original_wallpaper = None);
        }
        crate::logln!("wallpaper: restored '{original}' (ok={ok})");
        return ok;
    }

    let Some(asset) = asset_name(which) else {
        return false;
    };
    let Some(path) = extract(app, asset) else {
        crate::logln!("wallpaper: could not unpack {asset}");
        return false;
    };

    // Record what was there first, and only once, so repeatedly switching between our
    // own two wallpapers does not overwrite the user's original with one of ours.
    if crate::settings::load().original_wallpaper.is_none() {
        if let Some(before) = current() {
            crate::settings::update(|s| s.original_wallpaper = Some(before));
        }
    }

    let ok = apply(&path.to_string_lossy());
    crate::logln!("wallpaper: set {} (ok={ok})", path.display());
    ok
}
