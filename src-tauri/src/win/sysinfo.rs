//! CPU and memory load for the system-stats widget.
//!
//! No `sysinfo` crate: CPU load is two calls to `GetSystemTimes` with a stored
//! previous sample, and memory is one call to `GlobalMemoryStatusEx`. That is the
//! whole dependency, and it keeps the widget's cost to a few microseconds a second.

use std::sync::Mutex;

use serde::Serialize;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SysLoad {
    /// Whole-machine CPU use, 0..100. `None` until the second sample.
    pub cpu_percent: Option<u8>,
    pub ram_percent: u8,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
}

fn ticks(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}

/// Previous (idle, kernel, user) sample. CPU load is a delta, so the first call
/// after startup has nothing to compare against and reports `None`.
static PREV: Mutex<Option<(u64, u64, u64)>> = Mutex::new(None);

pub fn load() -> SysLoad {
    let mut out = SysLoad::default();

    unsafe {
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() {
            let now = (ticks(idle), ticks(kernel), ticks(user));
            let mut prev = PREV.lock().unwrap();
            if let Some(old) = *prev {
                // Kernel time INCLUDES idle time, so total is kernel + user and the
                // busy fraction is 1 - idle/total. Subtracting idle from kernel first
                // is the classic off-by-a-lot in this calculation.
                let d_idle = now.0.saturating_sub(old.0);
                let d_total = now.1.saturating_sub(old.1) + now.2.saturating_sub(old.2);
                if d_total > 0 {
                    let busy = 100.0 * (1.0 - d_idle as f64 / d_total as f64);
                    out.cpu_percent = Some(busy.clamp(0.0, 100.0).round() as u8);
                }
            }
            *prev = Some(now);
        }

        let mut mem = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if GlobalMemoryStatusEx(&mut mem).is_ok() {
            let total = mem.ullTotalPhys as f64;
            let used = total - mem.ullAvailPhys as f64;
            out.ram_percent = mem.dwMemoryLoad.min(100) as u8;
            out.ram_total_gb = (total / 1024.0 / 1024.0 / 1024.0) as f32;
            out.ram_used_gb = (used / 1024.0 / 1024.0 / 1024.0) as f32;
        }
    }

    out
}
