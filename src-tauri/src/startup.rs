//! Per-user "start when I sign in" entry.
//!
//! This is a shortcut in the user's own Startup folder rather than an `HKCU\...\Run`
//! value. Both are per-user and need no admin rights, but the shortcut keeps the
//! promise made in SYSTEM_CHANGES.md that this project writes **no registry keys at
//! all**, it is visible to the user in Explorer and in Task Manager > Startup, and
//! undoing it is deleting one file.

use std::path::{Path, PathBuf};

/// Filename `restore-windows.ps1` also looks for; keep the two in step.
const LNK_NAME: &str = "macui.lnk";

fn startup_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup"),
    )
}

pub fn lnk_path() -> Option<PathBuf> {
    Some(startup_dir()?.join(LNK_NAME))
}

pub fn is_enabled() -> bool {
    lnk_path().map(|p| p.is_file()).unwrap_or(false)
}

/// Create or remove the startup shortcut. Idempotent either way.
pub fn set(enabled: bool) -> std::io::Result<()> {
    let path = lnk_path().ok_or_else(|| std::io::Error::other("APPDATA is not set"))?;

    if !enabled {
        if path.is_file() {
            std::fs::remove_file(&path)?;
            crate::logln!("startup: removed {}", path.display());
        }
        return Ok(());
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let exe = std::env::current_exe()?;
    write_lnk(&exe, &path).map_err(|e| std::io::Error::other(e.to_string()))?;
    crate::logln!("startup: wrote {} -> {}", path.display(), exe.display());
    Ok(())
}

/// Write a `.lnk` pointing at `target`, the same way Explorer's "Create shortcut" does.
fn write_lnk(target: &Path, lnk: &Path) -> windows::core::Result<()> {
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    // Commands can run off the main thread, and COM is per-thread. Idempotent.
    crate::win::init_com();

    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;

        let t = HSTRING::from(target.as_os_str());
        link.SetPath(PCWSTR(t.as_ptr()))?;
        if let Some(dir) = target.parent() {
            let d = HSTRING::from(dir.as_os_str());
            link.SetWorkingDirectory(PCWSTR(d.as_ptr()))?;
        }
        link.SetDescription(windows::core::w!("macui - macOS-inspired UI layer"))?;

        let persist: IPersistFile = link.cast()?;
        let p = HSTRING::from(lnk.as_os_str());
        persist.Save(PCWSTR(p.as_ptr()), true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn lnk_path_is_under_the_users_startup_folder() {
        // Guards the one thing that would be silently wrong: writing the shortcut
        // somewhere Windows does not read, or somewhere shared between users.
        let p = super::lnk_path().expect("APPDATA is always set on Windows");
        let s = p.to_string_lossy().to_lowercase();
        assert!(s.ends_with(r"\startup\macui.lnk"), "unexpected path: {s}");
        assert!(s.contains(r"\appdata\roaming\"), "not per-user: {s}");
    }
}
