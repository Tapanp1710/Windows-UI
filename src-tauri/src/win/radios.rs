//! Wi-Fi and Bluetooth, through `Windows.Devices.Radios`.
//!
//! This is the same switch the Windows Quick Settings tiles throw: it turns the radio
//! itself on or off, rather than disconnecting or faking anything. Nothing is
//! intercepted and no driver is touched.
//!
//! **Everything here runs on one background thread** initialised multi-threaded, and
//! the UI only ever reads a snapshot of plain data - the same shape `media.rs` uses,
//! for the same reason and now also for a second one:
//!
//! 1. These are WinRT async calls, and blocking on `.get()` from the app's
//!    apartment-threaded main thread waits for a completion that needs that same
//!    thread to pump messages - a deadlock, not a slow call.
//! 2. `RequestAccessAsync`, `GetRadiosAsync` and especially `SetStateAsync` talk to
//!    the radio management service and the adapter itself. Switching a Bluetooth
//!    radio routinely takes seconds. The previous version ran that round trip inside
//!    a *synchronous* Tauri command - that is, on the main thread - so the window
//!    stopped pumping messages for the duration and Windows logged the process as
//!    `AppHangB1`, "stopped interacting with Windows", and closed it. See D-033.
//!
//! So `list()` returns the last snapshot and `set()` queues a write. Neither blocks.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RadioInfo {
    /// "wifi" or "bluetooth".
    pub kind: String,
    pub on: bool,
    /// False when Windows will not let us change it, so the UI can show the switch
    /// as read-only instead of silently doing nothing.
    pub can_toggle: bool,
    pub name: String,
}

/// What the poller last saw. `None` until the first read completes.
static SNAPSHOT: Mutex<Option<Vec<RadioInfo>>> = Mutex::new(None);
/// Writes waiting for the poller thread, oldest first.
static PENDING: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

/// The last known radio states. Never blocks; empty until the first poll lands.
pub fn list() -> Vec<RadioInfo> {
    SNAPSHOT.lock().unwrap().clone().unwrap_or_default()
}

/// Bluetooth presence and state, read from the same snapshot.
///
/// This used to shell out to PowerShell for `Get-PnpDevice -Class Bluetooth` from
/// the main thread, behind a `OnceLock`. On a machine under memory pressure a cold
/// PowerShell start is several seconds, and those seconds were spent not pumping
/// messages - the single biggest cause of the hangs. The radio list already carries
/// presence *and* the real soft on/off state, which the old version could only guess
/// at by reporting presence twice.
pub fn bluetooth() -> (bool, bool) {
    match list().into_iter().find(|r| r.kind == "bluetooth") {
        Some(r) => (true, r.on),
        None => (false, false),
    }
}

/// Queue a radio change. Returns at once; the poller applies it.
///
/// The snapshot is updated optimistically so the panel does not flick back to the
/// old state in the moment between the click and the radio actually switching. If
/// the write fails, the next poll puts the truth back within a couple of seconds.
pub fn set(kind: &str, on: bool) {
    PENDING.lock().unwrap().push((kind.to_string(), on));
    if let Some(list) = SNAPSHOT.lock().unwrap().as_mut() {
        if let Some(r) = list.iter_mut().find(|r| r.kind == kind) {
            r.on = on;
        }
    }
    crate::logln!("radio {kind} -> {on} (queued)");
}

/// Turn every radio on or off at once - what the Airplane mode tile does.
///
/// Windows' own airplane mode is a system state with no public API to set, so this
/// does the part that is actually ours to do: the radios themselves. The tile reads
/// as on when every radio present is off.
pub fn set_all(on: bool) {
    for r in list() {
        if r.can_toggle {
            set(&r.kind, on);
        }
    }
}

fn kind_name(kind: windows::Devices::Radios::RadioKind) -> Option<&'static str> {
    use windows::Devices::Radios::RadioKind;
    match kind {
        RadioKind::WiFi => Some("wifi"),
        RadioKind::Bluetooth => Some("bluetooth"),
        _ => None,
    }
}

fn read() -> windows::core::Result<Vec<RadioInfo>> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};

    // Desktop apps are normally granted this outright, but asking is required
    // before any radio can be changed.
    let access = Radio::RequestAccessAsync()?.get()?;
    let can_toggle = access == RadioAccessStatus::Allowed;

    let mut out = Vec::new();
    for radio in Radio::GetRadiosAsync()?.get()? {
        let Some(kind) = kind_name(radio.Kind()?) else {
            continue;
        };
        let state = radio.State()?;
        out.push(RadioInfo {
            kind: kind.to_string(),
            on: state == RadioState::On,
            // A radio Windows reports as Disabled or Unknown cannot be driven.
            can_toggle: can_toggle && (state == RadioState::On || state == RadioState::Off),
            name: radio.Name().map(|n| n.to_string()).unwrap_or_default(),
        });
    }
    Ok(out)
}

fn write(kind: &str, on: bool) -> windows::core::Result<bool> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};

    if Radio::RequestAccessAsync()?.get()? != RadioAccessStatus::Allowed {
        return Ok(false);
    }
    let want = if on { RadioState::On } else { RadioState::Off };

    for radio in Radio::GetRadiosAsync()?.get()? {
        if kind_name(radio.Kind()?) != Some(kind) {
            continue;
        }
        let status = radio.SetStateAsync(want)?.get()?;
        return Ok(status == RadioAccessStatus::Allowed);
    }
    Ok(false)
}

/// Start the poller. Call once, at startup, beside `media::start()`.
pub fn start() {
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            // MTA: see the module comment. S_FALSE (already initialised) is fine.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        crate::logln!("radio poller started");

        loop {
            // Writes first, so a click is acted on without waiting out a poll.
            let queued: Vec<(String, bool)> = std::mem::take(&mut *PENDING.lock().unwrap());
            for (kind, on) in queued {
                match write(&kind, on) {
                    Ok(ok) => crate::logln!("radio {kind} -> {on} (applied, ok={ok})"),
                    Err(e) => crate::logln!("radio {kind} -> {on} failed: {e}"),
                }
            }

            if let Ok(next) = read() {
                let mut slot = SNAPSHOT.lock().unwrap();
                if slot.as_ref() != Some(&next) {
                    *slot = Some(next);
                }
            }

            std::thread::sleep(Duration::from_millis(2000));
        }
    });
}
