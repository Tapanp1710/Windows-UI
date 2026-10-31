//! Available Wi-Fi networks, and joining one you already know.
//!
//! `wlanapi` is the same source the Windows network flyout reads. `WlanGetAvailable
//! NetworkList` returns the last scan the system did — Windows rescans on its own
//! while the flyout is open, and asking for a *fresh* scan (`WlanScan`) is an async
//! notification dance that takes seconds, so this reads the cached list. That is why
//! the list can take a moment to settle after Wi-Fi is switched on.
//!
//! **Joining is deliberately limited to networks with a saved profile.** Connecting
//! to a new one means building a profile XML containing the passphrase, which means
//! prompting for and holding a password — a credential surface this project has no
//! business growing. A network with no profile sends you to Windows' own flyout,
//! which already does it properly.
//!
//! Everything here runs off the main thread (the commands are async), for the reason
//! in D-033: `WlanOpenHandle` and friends are cross-process calls into the WLAN
//! service and are not reliably fast.

use serde::Serialize;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    WlanCloseHandle, WlanConnect, WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory,
    WlanGetAvailableNetworkList, WlanOpenHandle, dot11_BSS_type_infrastructure,
    wlan_connection_mode_profile, WLAN_AVAILABLE_NETWORK, WLAN_AVAILABLE_NETWORK_LIST,
    WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST,
};

/// `WLAN_AVAILABLE_NETWORK.dwFlags`.
const CONNECTED: u32 = 0x0000_0001;
const HAS_PROFILE: u32 = 0x0000_0002;
/// `WlanGetAvailableNetworkList` flags: fold hidden/ad-hoc entries in sensibly.
const INCLUDE_ALL_MANUAL_HIDDEN_PROFILES: u32 = 0x0000_0002;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0-100, as Windows reports it.
    pub signal: u32,
    pub secured: bool,
    pub connected: bool,
    /// We can join this one ourselves; without a profile it needs Windows' flyout.
    pub known: bool,
}

/// RAII for the WLAN client handle, so every early return still closes it.
struct Client(HANDLE);

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.0, None);
        }
    }
}

fn open() -> Option<Client> {
    unsafe {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        // Client version 2 is Vista and later, i.e. everything.
        if WlanOpenHandle(2, None, &mut version, &mut handle) != 0 {
            return None;
        }
        Some(Client(handle))
    }
}

/// The GUID of the first Wi-Fi interface. Machines with two are rare and the flyout
/// only ever drives one.
fn first_interface(c: &Client) -> Option<windows::core::GUID> {
    unsafe {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(c.0, None, &mut list) != 0 || list.is_null() {
            return None;
        }
        let guid = if (*list).dwNumberOfItems == 0 {
            None
        } else {
            Some((*list).InterfaceInfo[0].InterfaceGuid)
        };
        WlanFreeMemory(list as *const _);
        guid
    }
}

fn ssid_of(n: &WLAN_AVAILABLE_NETWORK) -> String {
    let len = (n.dot11Ssid.uSSIDLength as usize).min(32);
    String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).into_owned()
}

/// Every visible network, strongest first, one entry per SSID.
pub fn scan() -> Vec<WifiNetwork> {
    let Some(c) = open() else {
        return Vec::new();
    };
    let Some(guid) = first_interface(&c) else {
        return Vec::new();
    };

    let mut out: Vec<WifiNetwork> = Vec::new();
    unsafe {
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        if WlanGetAvailableNetworkList(
            c.0,
            &guid,
            INCLUDE_ALL_MANUAL_HIDDEN_PROFILES,
            None,
            &mut list,
        ) != 0
            || list.is_null()
        {
            return Vec::new();
        }

        // The struct declares one element and is allocated with however many there
        // are, so the rest are read by offset from the first.
        let count = (*list).dwNumberOfItems as usize;
        let first = (*list).Network.as_ptr();
        for i in 0..count {
            let n = &*first.add(i);
            let ssid = ssid_of(n);
            if ssid.is_empty() {
                continue; // a hidden network with nothing to show
            }
            let entry = WifiNetwork {
                ssid,
                signal: n.wlanSignalQuality,
                secured: n.bSecurityEnabled.as_bool(),
                connected: n.dwFlags & CONNECTED != 0,
                known: n.dwFlags & HAS_PROFILE != 0,
            };
            // The same SSID appears once per BSSID/security combination; keep the
            // strongest, and never let a duplicate hide the one we are joined to.
            match out.iter_mut().find(|e| e.ssid == entry.ssid) {
                Some(existing) => {
                    existing.connected |= entry.connected;
                    existing.known |= entry.known;
                    if entry.signal > existing.signal {
                        existing.signal = entry.signal;
                    }
                }
                None => out.push(entry),
            }
        }
        WlanFreeMemory(list as *const _);
    }

    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    out
}

/// Join a network that already has a saved profile. Returns false otherwise.
pub fn connect(ssid: &str) -> bool {
    let Some(c) = open() else {
        return false;
    };
    let Some(guid) = first_interface(&c) else {
        return false;
    };

    // Refuse anything we have no profile for rather than failing deep inside
    // WlanConnect with a code the UI cannot explain.
    if !scan().iter().any(|n| n.ssid == ssid && n.known) {
        crate::logln!("wifi: no saved profile for {ssid:?}; leaving it to Windows");
        return false;
    }

    let profile: Vec<u16> = ssid.encode_utf16().chain(std::iter::once(0)).collect();
    let params = WLAN_CONNECTION_PARAMETERS {
        // Connect by profile name: the profile holds the credentials, so none pass
        // through this process.
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: windows::core::PCWSTR(profile.as_ptr()),
        pDot11Ssid: std::ptr::null_mut(),
        pDesiredBssidList: std::ptr::null_mut(),
        dot11BssType: dot11_BSS_type_infrastructure,
        dwFlags: 0,
    };

    let ok = unsafe { WlanConnect(c.0, &guid, &params, None) } == 0;
    crate::logln!("wifi: connect to {ssid:?} -> {ok}");
    ok
}

pub fn disconnect() -> bool {
    let Some(c) = open() else {
        return false;
    };
    let Some(guid) = first_interface(&c) else {
        return false;
    };
    let ok = unsafe { WlanDisconnect(c.0, &guid, None) } == 0;
    crate::logln!("wifi: disconnect -> {ok}");
    ok
}
