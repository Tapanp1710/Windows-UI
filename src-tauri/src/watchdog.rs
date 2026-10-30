//! Crash insurance for the Windows taskbar.
//!
//! The main process hides the taskbar. If it exits cleanly it unhides it again. But
//! "End task" in Task Manager, a panic with `panic = "abort"`, or a power event can all
//! kill it without any cleanup running, and that would leave the user with no taskbar
//! and no obvious way back.
//!
//! So on startup the app re-launches its own executable with
//! `--watchdog <parent-pid> <restore-state>`. That second process does nothing but
//! block on the parent's process handle. When the parent dies — for any reason, clean
//! or not — the wait returns and the watchdog restores the taskbar and exits.
//!
//! Cost: one extra process that is blocked in the kernel the whole time. No polling, no
//! timer, no measurable CPU, and about 1 MB of working set.
//!
//! The one case this cannot cover is the watchdog itself being killed as well (e.g.
//! killing the whole process tree). `restore-windows.ps1` is the manual backstop for
//! that, and it is documented in README/REPORT.

use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, WAIT_FAILED};
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};

use crate::win::taskbar;

/// Argument marker. Kept in one place so `main` and the spawner cannot drift apart.
pub const FLAG: &str = "--watchdog";

/// Runs the watchdog. Never returns to normal app startup.
///
/// `parent_pid` is the process to watch; `restore_state` is the raw `ABS_*` taskbar
/// state captured before anything was hidden, so the restore puts back what the user
/// actually had rather than a guessed default.
pub fn run(parent_pid: u32, restore_state: Option<u32>) {
    unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
            Ok(handle) if !handle.is_invalid() => {
                // Blocks in the kernel until the parent terminates. Zero CPU.
                let r = WaitForSingleObject(handle, INFINITE);
                let _ = CloseHandle(handle);
                if r == WAIT_FAILED {
                    // Could not wait properly; fall through and restore anyway. A
                    // spurious restore is harmless, a missed one is not.
                }
            }
            _ => {
                // The parent was already gone before we could open it. Restore
                // immediately rather than exiting and leaving the taskbar hidden.
            }
        }
    }

    // Give a clean shutdown a moment to do its own restore first, so we are not
    // fighting it over the same window.
    std::thread::sleep(Duration::from_millis(250));

    crate::log::init("watchdog");
    crate::logln!("parent {parent_pid} gone; restoring taskbar");
    taskbar::full_restore(restore_state);
}

/// Spawn the watchdog as a detached child of this process.
///
/// Returns the child handle so the app can kill it during a clean shutdown — otherwise
/// the watchdog would fire a redundant restore every time the app quits normally.
pub fn spawn(restore_state: Option<u32>) -> std::io::Result<std::process::Child> {
    let exe = std::env::current_exe()?;
    let pid = std::process::id();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(FLAG).arg(pid.to_string());
    if let Some(s) = restore_state {
        cmd.arg(s.to_string());
    }

    // CREATE_NO_WINDOW: never flash a console, even from a debug build.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.spawn()
}

/// Parse `--watchdog <pid> [state]` out of the command line.
///
/// Returns `None` for a normal app launch.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Option<(u32, Option<u32>)> {
    let args: Vec<String> = args.into_iter().collect();
    let i = args.iter().position(|a| a == FLAG)?;
    let pid = args.get(i + 1)?.parse::<u32>().ok()?;
    let state = args.get(i + 2).and_then(|s| s.parse::<u32>().ok());
    Some((pid, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_watchdog_invocation() {
        assert_eq!(
            parse_args(v(&["macui.exe", "--watchdog", "1234", "1"])),
            Some((1234, Some(1)))
        );
        // State is optional.
        assert_eq!(
            parse_args(v(&["macui.exe", "--watchdog", "1234"])),
            Some((1234, None))
        );
        // A normal launch must not be mistaken for a watchdog launch, or the app
        // would silently never start.
        assert_eq!(parse_args(v(&["macui.exe"])), None);
        assert_eq!(parse_args(v(&["macui.exe", "--settings"])), None);
        // Garbage pid is not a watchdog launch either.
        assert_eq!(parse_args(v(&["macui.exe", "--watchdog", "abc"])), None);
    }
}
