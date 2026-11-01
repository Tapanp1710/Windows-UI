//! Three-finger swipe on a precision touchpad opens the Alt+Tab carousel.
//!
//! Swipe three fingers sideways and the carousel opens; keep sliding and it turns,
//! one window per stretch of travel (right = next, left = back); lift the fingers
//! and you switch, exactly as letting go of Alt does.
//!
//! The touchpad is read through raw input: a message-only window registers for the
//! Precision Touchpad HID collection (Digitizer page 0x0D, usage 0x05) with
//! RIDEV_INPUTSINK, so the reports arrive whichever window has focus. That is a
//! read of reports Windows already produces - it sits in nobody's input path, and
//! the pointer and Windows' own two-finger scrolling are untouched.
//!
//! Windows has its own three-finger "Switch apps" swipe on the same gesture, and
//! two switchers fighting over one swipe would be worse than either. So while this
//! is on, that one setting (HKCU ...\PrecisionTouchPad\ThreeFingerSlideEnabled) is
//! set to "Nothing", its old value kept in settings.json, and put back on every
//! exit path that runs `restore_windows_state` - see SYSTEM_CHANGES.md.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::core::w;
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input, HIDP_CAPS,
    HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDI_PREPARSEDDATA, RID_INPUT, RIM_TYPEHID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WM_INPUT,
    WS_OVERLAPPED,
};

use super::hotkeys::{self, Key};

/// Master switch, from settings.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Sideways travel, as a share of the pad's width, that turns the carousel by one.
const STEP: f32 = 0.085;

struct Device {
    preparsed: Vec<u8>,
    /// Link collections that carry a contact's X, with that axis's logical range.
    contacts: Vec<(u16, i32, i32)>,
}

impl Device {
    fn pp(&self) -> PHIDP_PREPARSED_DATA {
        PHIDP_PREPARSED_DATA(self.preparsed.as_ptr() as isize)
    }
}

fn load_device(handle: HANDLE) -> Option<Device> {
    unsafe {
        let mut size = 0u32;
        GetRawInputDeviceInfoW(handle, RIDI_PREPARSEDDATA, None, &mut size);
        if size == 0 {
            return None;
        }
        let mut preparsed = vec![0u8; size as usize];
        if GetRawInputDeviceInfoW(handle, RIDI_PREPARSEDDATA, Some(preparsed.as_mut_ptr().cast()), &mut size)
            == u32::MAX
        {
            return None;
        }
        let pp = PHIDP_PREPARSED_DATA(preparsed.as_ptr() as isize);
        let mut caps = HIDP_CAPS::default();
        if HidP_GetCaps(pp, &mut caps) != HIDP_STATUS_SUCCESS {
            return None;
        }
        let mut n = caps.NumberInputValueCaps;
        let mut values = vec![HIDP_VALUE_CAPS::default(); n as usize];
        if HidP_GetValueCaps(HidP_Input, values.as_mut_ptr(), &mut n, pp) != HIDP_STATUS_SUCCESS {
            return None;
        }
        let contacts = values[..n as usize]
            .iter()
            .filter(|v| v.UsagePage == 0x01 && !v.IsRange.as_bool() && v.Anonymous.NotRange.Usage == 0x30)
            .map(|v| (v.LinkCollection, v.LogicalMin, v.LogicalMax))
            .collect();
        Some(Device { preparsed, contacts })
    }
}

/// One finger in one report: its id, X as a share of the pad's width, and whether
/// it is touching.
type Contact = (u32, f32, bool);

fn parse(dev: &Device, report: &mut [u8]) -> (Option<u32>, Vec<Contact>) {
    let pp = dev.pp();
    let mut out = Vec::new();
    let count;
    unsafe {
        let mut n = 0u32;
        count = (HidP_GetUsageValue(HidP_Input, 0x0D, 0, 0x54, &mut n, pp, report)
            == HIDP_STATUS_SUCCESS)
            .then_some(n);
        for &(link, min, max) in &dev.contacts {
            let mut x = 0u32;
            if HidP_GetUsageValue(HidP_Input, 0x01, link, 0x30, &mut x, pp, report) != HIDP_STATUS_SUCCESS {
                continue;
            }
            let mut id = 0u32;
            let _ = HidP_GetUsageValue(HidP_Input, 0x0D, link, 0x51, &mut id, pp, report);
            let mut usages = [0u16; 16];
            let mut len = usages.len() as u32;
            let tip = HidP_GetUsages(HidP_Input, 0x0D, link, usages.as_mut_ptr(), &mut len, pp, report)
                == HIDP_STATUS_SUCCESS
                && usages[..len as usize].contains(&0x42);
            let span = (max - min).max(1) as f32;
            out.push((id, (x as i32 - min) as f32 / span, tip));
        }
    }
    (count, out)
}

/// When the last frame arrived, in ms, so a lost "lifted" frame cannot hold the
/// carousel open: `hotkeys::switch_held` stops trusting a gesture that has gone
/// quiet. A touchpad reports continuously while fingers rest on it.
pub static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Gesture {
    active: bool,
    opened: bool,
    anchor: f32,
}

impl Gesture {
    fn frame(&mut self, contacts: &[Contact]) {
        let down: Vec<f32> = contacts.iter().filter(|c| c.2).map(|c| c.1).collect();
        if down.len() >= 3 && ENABLED.load(Ordering::Relaxed) {
            let mean = down.iter().sum::<f32>() / down.len() as f32;
            if !self.active {
                crate::logln!("touchpad: three fingers down");
                *self = Gesture { active: true, opened: false, anchor: mean };
                return;
            }
            let dx = mean - self.anchor;
            if dx.abs() >= STEP {
                let back = dx < 0.0;
                if !self.opened {
                    self.opened = true;
                    crate::logln!("touchpad: three-finger swipe -> switcher");
                    hotkeys::GESTURE_HELD.store(true, Ordering::SeqCst);
                    hotkeys::SWITCHER_OPEN.store(true, Ordering::SeqCst);
                }
                hotkeys::send_key(Key::Switch(back));
                self.anchor += if back { -STEP } else { STEP };
            }
        } else if self.active {
            // Fingers lifted: switch to the selection, like letting go of Alt.
            let opened = self.opened;
            *self = Gesture::default();
            if opened {
                hotkeys::GESTURE_HELD.store(false, Ordering::SeqCst);
                hotkeys::send_key(Key::Commit);
            }
        }
    }
}

/// Start listening, on a thread of its own. Does nothing on a machine without a
/// precision touchpad - registering simply delivers no reports.
pub fn start() {
    std::thread::spawn(|| unsafe {
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("macui touchpad"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            None,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                crate::logln!("touchpad: no message window ({e})");
                return;
            }
        };
        let rid = [RAWINPUTDEVICE {
            usUsagePage: 0x0D,
            usUsage: 0x05,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        }];
        if let Err(e) = RegisterRawInputDevices(&rid, std::mem::size_of::<RAWINPUTDEVICE>() as u32) {
            crate::logln!("touchpad: raw input not registered ({e})");
            return;
        }
        crate::logln!("touchpad: listening for three-finger swipes");

        let mut devices: HashMap<isize, Option<Device>> = HashMap::new();
        let mut gesture = Gesture::default();
        // Hybrid-mode pads send one finger per report: the first carries the count,
        // the rest carry 0, and a frame is complete once that many have arrived.
        let mut frame: Vec<Contact> = Vec::new();
        let mut expected = 0usize;
        let mut buf: Vec<u8> = Vec::new();

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_INPUT {
                let raw = HRAWINPUT(msg.lParam.0 as *mut _);
                let mut size = 0u32;
                let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
                GetRawInputData(raw, RID_INPUT, None, &mut size, header);
                buf.resize(size as usize, 0);
                if size > 0 && GetRawInputData(raw, RID_INPUT, Some(buf.as_mut_ptr().cast()), &mut size, header) == size {
                    let input = &*(buf.as_ptr() as *const RAWINPUT);
                    if input.header.dwType == RIM_TYPEHID.0 {
                        let key = input.header.hDevice.0 as isize;
                        let dev = devices.entry(key).or_insert_with(|| load_device(input.header.hDevice));
                        if let Some(dev) = dev {
                            let hid = &input.data.hid;
                            let each = hid.dwSizeHid as usize;
                            let start = (&hid.bRawData as *const u8).offset_from(buf.as_ptr()) as usize;
                            for i in 0..hid.dwCount as usize {
                                let a = start + i * each;
                                let Some(slice) = buf.get(a..a + each) else { break };
                                let mut report = slice.to_vec();
                                let (count, contacts) = parse(dev, &mut report);
                                if let Some(c) = count.filter(|&c| c > 0) {
                                    frame.clear();
                                    expected = c as usize;
                                }
                                frame.extend(contacts);
                                if expected > 0 && frame.len() >= expected {
                                    LAST_FRAME_MS.store(hotkeys::now_ms(), Ordering::Relaxed);
                                    gesture.frame(&frame);
                                    frame.clear();
                                    expected = 0;
                                }
                            }
                        }
                    }
                }
            }
            DispatchMessageW(&msg);
        }
    });
}

/* ------------------------------------------------- Windows' own swipe */

const PTP_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\PrecisionTouchPad");
const SLIDE: windows::core::PCWSTR = w!("ThreeFingerSlideEnabled");

fn read_slide() -> Option<u32> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut v = 0u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PTP_KEY, SLIDE, RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut size))
    };
    ok.is_ok().then_some(v)
}

fn write_slide(value: Option<u32>) {
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD};
    unsafe {
        let _ = match value {
            Some(v) => RegSetKeyValueW(HKEY_CURRENT_USER, PTP_KEY, SLIDE, REG_DWORD.0, Some((&v as *const u32).cast()), 4),
            None => RegDeleteKeyValueW(HKEY_CURRENT_USER, PTP_KEY, SLIDE),
        };
    }
    notify_setting_change();
}

fn notify_setting_change() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("PrecisionTouchPad").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            None,
        );
    }
}

/// Take Windows' three-finger swipe off the gesture, remembering what it was.
/// The old value goes into settings.json first, so a crash between the two leaves
/// something to restore from. `-1` there means "was not set" (Windows' default).
pub fn claim_windows_swipe() {
    let s = crate::settings::load();
    if s.three_finger_saved.is_none() {
        let was = read_slide().map_or(-1, |v| v as i64);
        crate::settings::update(|s| s.three_finger_saved = Some(was));
        crate::logln!("touchpad: Windows' three-finger swipe was {was}; set to Nothing");
    }
    if read_slide() != Some(0) {
        write_slide(Some(0));
    }
}

/// Put Windows' three-finger swipe back exactly as it was. Safe to call when it
/// was never taken.
pub fn release_windows_swipe() {
    let saved = crate::settings::load().three_finger_saved;
    if let Some(was) = saved {
        write_slide(u32::try_from(was).ok());
        crate::settings::update(|s| s.three_finger_saved = None);
        crate::logln!("touchpad: Windows' three-finger swipe restored ({was})");
    }
}
