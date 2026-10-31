//! Wi-Fi and Bluetooth, through `Windows.Devices.Radios`.
//!
//! This is the same switch the Windows Quick Settings tiles throw: it turns the radio
//! itself on or off, rather than disconnecting or faking anything. Nothing is
//! intercepted and no driver is touched.
//!
//! Like the media session, these are WinRT async calls, so they run on a short-lived
//! **multi-threaded** apartment and the caller waits for the result. Blocking on
//! `.get()` from the app's apartment-threaded main thread would wait for a completion
//! that needs that same thread to pump messages - a deadlock, not a slow call.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
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

/// Run a closure on a fresh MTA thread and wait for it.
///
/// Cheap enough for user-initiated actions and for a panel that polls a couple of
/// times a second; a thread spawn is microseconds next to the WinRT round trip.
fn mta<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        f()
    })
    .join()
    .ok()
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

pub fn list() -> Vec<RadioInfo> {
    mta(|| read().unwrap_or_default()).unwrap_or_default()
}

fn write(kind: String, on: bool) -> windows::core::Result<bool> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};

    if Radio::RequestAccessAsync()?.get()? != RadioAccessStatus::Allowed {
        return Ok(false);
    }
    let want = if on { RadioState::On } else { RadioState::Off };

    for radio in Radio::GetRadiosAsync()?.get()? {
        if kind_name(radio.Kind()?) != Some(kind.as_str()) {
            continue;
        }
        let status = radio.SetStateAsync(want)?.get()?;
        return Ok(status == RadioAccessStatus::Allowed);
    }
    Ok(false)
}

/// `kind` is "wifi" or "bluetooth".
pub fn set(kind: &str, on: bool) -> bool {
    let k = kind.to_string();
    let ok = mta(move || write(k, on).unwrap_or(false)).unwrap_or(false);
    crate::logln!("radio {kind} -> {on} (ok={ok})");
    ok
}
