//! Paired Bluetooth devices and whether they are connected.
//!
//! `BluetoothFindFirstDevice` over the remembered/authenticated set is the same list
//! the Windows Bluetooth page shows, and `fConnected` is the same live state. No
//! inquiry is issued — `fIssueInquiry: false` means this reads the paired list rather
//! than scanning the air, which would take ten seconds and light up the radio.
//!
//! **Connecting is not done here.** There is no general "connect this device" call:
//! it is per-profile (`BluetoothSetServiceState` with an A2DP or HFP or HID GUID),
//! and getting it wrong for a device class leaves a half-connected device that the
//! Windows UI then disagrees about. Tapping a device opens Windows' own Bluetooth
//! page, which knows each device's profiles. The list and the live state are the part
//! worth having in the panel; see D-050.
//!
//! Runs off the main thread (the command is async): this is a cross-process call
//! into the Bluetooth service.

use serde::Serialize;
use windows::Win32::Devices::Bluetooth::{
    BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindNextDevice,
    BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS,
};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    pub name: String,
    pub connected: bool,
    /// "audio", "phone", "keyboard", "mouse", "computer" or "other" — picked from the
    /// class-of-device bits so the row can carry a recognisable glyph.
    pub kind: String,
}

/// Major device class lives in bits 8..12 of `ulClassofDevice`.
fn kind_of(class: u32) -> &'static str {
    let major = (class >> 8) & 0x1F;
    let minor = (class >> 2) & 0x3F;
    match major {
        0x01 => "computer",
        0x02 => "phone",
        0x04 => "audio",
        0x05 => match minor {
            // Peripheral minor class: bits 6..7 of the minor field.
            m if m & 0x10 != 0 => "keyboard",
            m if m & 0x20 != 0 => "mouse",
            _ => "other",
        },
        _ => "other",
    }
}

fn name_of(info: &BLUETOOTH_DEVICE_INFO) -> String {
    let end = info
        .szName
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(info.szName.len());
    String::from_utf16_lossy(&info.szName[..end])
        .trim()
        .to_string()
}

/// Every paired device, connected ones first.
pub fn devices() -> Vec<BtDevice> {
    let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: true.into(),
        fReturnRemembered: true.into(),
        fReturnUnknown: false.into(),
        fReturnConnected: true.into(),
        // No radio inquiry: read the paired list, do not scan the air.
        fIssueInquiry: false.into(),
        cTimeoutMultiplier: 0,
        // Null radio handle means "every radio on this machine".
        hRadio: Default::default(),
    };

    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };

    let mut out: Vec<BtDevice> = Vec::new();
    unsafe {
        let Ok(find) = BluetoothFindFirstDevice(&params, &mut info) else {
            // No radio, or none paired. Both are "nothing to show", not an error.
            return out;
        };

        loop {
            let name = name_of(&info);
            if !name.is_empty() {
                out.push(BtDevice {
                    name,
                    connected: info.fConnected.as_bool(),
                    kind: kind_of(info.ulClassofDevice).to_string(),
                });
            }
            // Reset the size field: the API wants it set on every call.
            info = BLUETOOTH_DEVICE_INFO {
                dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            if BluetoothFindNextDevice(find, &mut info).is_err() {
                break;
            }
        }
        let _ = BluetoothFindDeviceClose(find);
    }

    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Class-of-device is packed bit maths, and getting the shift wrong still
    /// returns a plausible-looking string. These are real values from the Bluetooth
    /// assigned-numbers table.
    #[test]
    fn device_class_decoding() {
        // Headphones / earbuds: major 0x04 (audio-video).
        assert_eq!(kind_of(0x24_0404), "audio");
        // Hands-free headset.
        assert_eq!(kind_of(0x24_0408), "audio");
        // Phone: major 0x02.
        assert_eq!(kind_of(0x5A_020C), "phone");
        // Computer: major 0x01.
        assert_eq!(kind_of(0x10_010C), "computer");
        // Peripheral (major 0x05) splits on the minor field.
        assert_eq!(kind_of(0x00_2540), "keyboard");
        assert_eq!(kind_of(0x00_2580), "mouse");
        // Anything unrecognised still names itself.
        assert_eq!(kind_of(0x00_0000), "other");
    }

    /// The enumeration must not panic and must not hang on a machine with no radio
    /// or nothing paired - both are ordinary, and both have to come back empty
    /// rather than erroring.
    #[test]
    fn enumerating_devices_is_safe() {
        let found = devices();
        for d in &found {
            assert!(!d.name.is_empty(), "a device was listed with no name");
        }
        // Connected devices must sort ahead of merely paired ones.
        let first_unconnected = found.iter().position(|d| !d.connected);
        if let Some(i) = first_unconnected {
            assert!(
                found[i..].iter().all(|d| !d.connected),
                "a connected device sorted below a disconnected one"
            );
        }
        eprintln!("paired devices seen: {}", found.len());
    }
}
