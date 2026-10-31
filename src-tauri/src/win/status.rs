//! Live system status for the top bar: battery, volume, network, focused app.
//!
//! Everything here is a cheap Win32 query. The top bar samples them on a timer rather
//! than registering for notifications, because the notification APIs for these
//! (WM_POWERBROADCAST, IMMNotificationClient, NLM connection points) each need a
//! message-pump window or a COM sink, and a 2-second poll of four tiny structs costs
//! less than keeping those alive. Measured idle cost stays at ~0%.

use serde::Serialize;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// 0..100, or None when there is no battery or the level is unknown.
    pub percent: Option<u8>,
    pub charging: bool,
    pub present: bool,
    /// Windows' own "saver on" flag, so the bar can mirror it.
    pub saver: bool,
}

pub fn battery() -> Battery {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe {
        if GetSystemPowerStatus(&mut s).is_err() {
            return Battery::default();
        }
    }

    // BatteryFlag bit 7 (128) means "no system battery".
    let present = s.BatteryFlag & 128 == 0 && s.BatteryLifePercent != 255;
    Battery {
        percent: (s.BatteryLifePercent != 255).then_some(s.BatteryLifePercent),
        // ACLineStatus 1 = on mains. BatteryFlag bit 3 (8) = charging.
        charging: s.ACLineStatus == 1 && (s.BatteryFlag & 8 != 0 || s.BatteryLifePercent < 100),
        present,
        saver: s.SystemStatusFlag == 1,
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// 0..100
    pub level: u8,
    pub muted: bool,
    pub available: bool,
}

/// Read the default playback device's master volume through the Core Audio API.
pub fn volume() -> Volume {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return Volume::default();
        };
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) else {
            // No playback device at all - the bar shows a muted icon rather than lying.
            return Volume::default();
        };
        let Ok(endpoint) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
            return Volume::default();
        };

        let scalar = endpoint.GetMasterVolumeLevelScalar().unwrap_or(0.0);
        let muted = endpoint.GetMute().map(|b| b.as_bool()).unwrap_or(false);

        Volume {
            level: (scalar * 100.0).round().clamp(0.0, 100.0) as u8,
            muted,
            available: true,
        }
    }
}

/// Nudge the master volume by `delta` percentage points, for scroll-on-the-icon.
///
/// Mirrors the Windows taskbar: scrolling changes the same master volume the taskbar
/// slider does, and unmutes nothing on its own.
pub fn set_volume(level: u8) -> bool {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return false;
        };
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) else {
            return false;
        };
        let Ok(endpoint) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
            return false;
        };
        let scalar = (level.min(100) as f32) / 100.0;
        endpoint
            .SetMasterVolumeLevelScalar(scalar, std::ptr::null())
            .is_ok()
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub connected: bool,
    /// True when the active connection is Wi-Fi rather than Ethernet.
    pub wifi: bool,
    /// 0..100 signal strength, None for Ethernet or when unknown.
    pub signal: Option<u8>,
    pub ssid: Option<String>,
}

/// Whether the machine has internet, via the Network List Manager.
///
/// Signal strength and SSID need the WLAN API (`wlanapi.dll`), which is a larger
/// surface; the bar shows a generic Wi-Fi glyph when connected wirelessly. Recorded
/// as a known limitation rather than faking a bar level.
pub fn network() -> Network {
    use windows::core::Interface;
    use windows::Win32::Networking::NetworkListManager::{
        INetworkListManager, NetworkListManager, NLM_CONNECTIVITY_IPV4_INTERNET,
        NLM_CONNECTIVITY_IPV6_INTERNET,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

    unsafe {
        let Ok(mgr) =
            CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
        else {
            return Network::default();
        };
        let connectivity = mgr.GetConnectivity().unwrap_or_default();
        let connected = connectivity.0
            & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0)
            != 0;
        let _ = mgr.cast::<INetworkListManager>();

        Network {
            connected,
            wifi: connected && has_wifi_adapter(),
            signal: None,
            ssid: None,
        }
    }
}

/// Cheap heuristic: does this machine have a wireless adapter at all?
fn has_wifi_adapter() -> bool {
    // Avoided a full WLAN enumeration here; the top bar only needs to pick a glyph.
    // Cached because the answer cannot change while the app runs.
    use std::sync::OnceLock;
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false)
    })
}

use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FocusedApp {
    pub name: String,
    pub icon: Option<String>,
}

/// Name and icon of the window the user is currently working in.
pub fn focused_app() -> FocusedApp {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_invalid() {
            return FocusedApp::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let Some(exe) = super::apps::exe_path_for_pid(pid) else {
            return FocusedApp::default();
        };

        // Our own windows should never take over the "focused app" pill.
        if exe
            .file_name()
            .map(|f| f.to_string_lossy().eq_ignore_ascii_case("macui.exe"))
            .unwrap_or(false)
        {
            return FocusedApp::default();
        }

        FocusedApp {
            name: exe
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            icon: None, // filled from the dock's icon cache by the caller
        }
    }
}

/// Open Windows' own Quick Settings flyout (the Win+A panel).
///
/// Synthesised rather than reimplemented: the brief requires clicking Wi-Fi, volume or
/// battery to open *the same* Windows panel, so the correct thing is to ask Windows to
/// show it, not to draw a replacement.
pub fn open_quick_settings() {
    super::press_win_key('A');
}

/// Open the Windows calendar / notification centre (Win+N).
pub fn open_notification_centre() {
    super::press_win_key('N');
}
