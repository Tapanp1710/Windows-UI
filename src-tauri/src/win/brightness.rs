//! Laptop panel brightness, through WMI.
//!
//! There is no Win32 call for an internal display's backlight. `SetMonitorBrightness`
//! in dxva2 speaks DDC/CI, which external monitors answer and built-in panels do not;
//! the panel's backlight lives in `root\wmi` as `WmiMonitorBrightness` (read) and
//! `WmiMonitorBrightnessMethods::WmiSetBrightness` (write). That is the same pair the
//! Windows brightness slider uses, so this moves the real backlight and Windows'
//! own slider follows it.
//!
//! Like the radios, **all of it runs on one background thread** and the UI reads a
//! cached value. WMI is a cross-process call into the WMI service; doing it inside a
//! synchronous Tauri command would put it on the main thread, which is the mistake
//! D-033 is about. The thread is idle except when a write is queued or the 30-second
//! re-read comes round, so it costs nothing at rest.
//!
//! Not every machine has this. Desktops with external monitors have no
//! `WmiMonitorBrightness` instance at all, and the level stays `None` so the panel
//! leaves the slider out rather than showing one that does nothing.

use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use windows::core::{BSTR, HSTRING, VARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, EOAC_NONE,
    RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

/// Last known level, 0-100. -1 means "not read yet or not supported".
static LEVEL: AtomicI32 = AtomicI32::new(-1);
/// Level the user asked for, or -1 for nothing pending.
static WANTED: AtomicI32 = AtomicI32::new(-1);

/// The current level, or `None` where the machine has no software-dimmable panel.
pub fn level() -> Option<u8> {
    match LEVEL.load(Ordering::Relaxed) {
        -1 => None,
        n => Some(n.clamp(0, 100) as u8),
    }
}

/// Queue a brightness change. Returns immediately.
pub fn set(percent: u8) {
    let p = percent.min(100) as i32;
    WANTED.store(p, Ordering::Relaxed);
    // Paint-ahead: the backlight takes a moment, and a slider that springs back to
    // where it was reads as broken.
    LEVEL.store(p, Ordering::Relaxed);
}

/// Connect to `root\wmi` with the impersonation WMI requires.
fn connect() -> windows::core::Result<IWbemServices> {
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let services = locator.ConnectServer(
            &BSTR::from(r"root\wmi"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )?;
        // Without this every call comes back E_ACCESSDENIED.
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;
        Ok(services)
    }
}

/// First object of a WQL query, or `None` when the class has no instances.
fn query_one(services: &IWbemServices, wql: &str) -> Option<IWbemClassObject> {
    unsafe {
        let e = services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(wql),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .ok()?;
        let mut objs: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0u32;
        if e.Next(WBEM_INFINITE, &mut objs, &mut returned).is_err() || returned == 0 {
            return None;
        }
        objs[0].take()
    }
}

/// Read a named property off a WMI object.
///
/// `TryFrom<&VARIANT>` goes through `VariantToUInt32`, which coerces, so this does
/// not care that `CurrentBrightness` arrives as VT_UI1 rather than an integer.
fn prop_u32(obj: &IWbemClassObject, name: &str) -> Option<u32> {
    unsafe {
        let mut v = VARIANT::default();
        obj.Get(&HSTRING::from(name), 0, &mut v, None, None).ok()?;
        u32::try_from(&v).ok()
    }
}

fn prop_bstr(obj: &IWbemClassObject, name: &str) -> Option<BSTR> {
    unsafe {
        let mut v = VARIANT::default();
        obj.Get(&HSTRING::from(name), 0, &mut v, None, None).ok()?;
        BSTR::try_from(&v).ok()
    }
}

fn read(services: &IWbemServices) -> Option<i32> {
    let obj = query_one(services, "SELECT * FROM WmiMonitorBrightness")?;
    prop_u32(&obj, "CurrentBrightness").map(|n| n as i32)
}

fn write(services: &IWbemServices, percent: i32) -> windows::core::Result<()> {
    unsafe {
        // The method lives on an instance, so its __PATH is needed to call it.
        let Some(inst) = query_one(services, "SELECT * FROM WmiMonitorBrightnessMethods") else {
            return Ok(());
        };
        let Some(path) = prop_bstr(&inst, "__PATH") else {
            return Ok(());
        };

        // Build the input parameter object from the method's own signature.
        let mut class: Option<IWbemClassObject> = None;
        services.GetObject(
            &BSTR::from("WmiMonitorBrightnessMethods"),
            Default::default(),
            None,
            Some(&mut class),
            None,
        )?;
        let class = class.ok_or_else(windows::core::Error::from_win32)?;

        let mut in_sig: Option<IWbemClassObject> = None;
        class.GetMethod(
            &HSTRING::from("WmiSetBrightness"),
            0,
            &mut in_sig,
            std::ptr::null_mut(),
        )?;
        let args = in_sig
            .ok_or_else(windows::core::Error::from_win32)?
            .SpawnInstance(0)?;

        // Both as VT_I4. WMI declares these `uint32` and `uint8`, but its VARIANT
        // marshalling wants a signed 32-bit integer for both and answers
        // WBEM_E_TYPE_MISMATCH (0x80041005) to the "obvious" VT_UI4/VT_UI1 pair.
        // Timeout is seconds to wait for the panel to acknowledge.
        args.Put(&HSTRING::from("Timeout"), 0, &VARIANT::from(1i32), 0)?;
        args.Put(
            &HSTRING::from("Brightness"),
            0,
            &VARIANT::from(percent.clamp(0, 100)),
            0,
        )?;

        services.ExecMethod(
            &path,
            &BSTR::from("WmiSetBrightness"),
            Default::default(),
            None,
            &args,
            None,
            None,
        )?;
        Ok(())
    }
}

/// Start the brightness thread. Call once, at startup.
pub fn start() {
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let services = match connect() {
            Ok(s) => s,
            Err(e) => {
                crate::logln!("brightness: no WMI connection ({e}); slider will be hidden");
                return;
            }
        };

        match read(&services) {
            Some(n) => {
                LEVEL.store(n, Ordering::Relaxed);
                crate::logln!("brightness: {n}% (software control available)");
            }
            None => {
                crate::logln!("brightness: no dimmable panel; slider will be hidden");
                return;
            }
        }

        let mut last_read = Instant::now();
        loop {
            let wanted = WANTED.swap(-1, Ordering::Relaxed);
            if wanted >= 0 {
                if let Err(e) = write(&services, wanted) {
                    crate::logln!("brightness -> {wanted}% failed: {e}");
                }
                last_read = Instant::now();
            } else if last_read.elapsed() >= Duration::from_secs(30) {
                // Catch changes made on the keyboard's own brightness keys.
                if let Some(n) = read(&services) {
                    LEVEL.store(n, Ordering::Relaxed);
                }
                last_read = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the real backlight: read it, set it somewhere else, read it back,
    /// put it back where it was.
    ///
    /// This is the only way to know the WMI plumbing works - `WmiSetBrightness`
    /// reports success through `ExecMethod` returning Ok, which it does even when
    /// the parameter object was built wrong. The check is that the panel actually
    /// moved.
    ///
    /// Skips cleanly on a machine with no dimmable panel, which is most desktops.
    /// It does briefly change the screen brightness, so it is a deliberate run:
    /// `cargo test brightness_round_trip -- --nocapture`.
    #[test]
    fn brightness_round_trip() {
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        let Ok(services) = connect() else {
            eprintln!("no WMI connection; skipping");
            return;
        };
        let Some(original) = read(&services) else {
            eprintln!("no dimmable panel on this machine; skipping");
            return;
        };

        // Somewhere clearly different, and never 0 - that blanks some panels.
        let target = if original > 55 { 30 } else { 80 };
        write(&services, target).expect("WmiSetBrightness failed");

        // The panel takes a moment to report the new level back.
        let mut seen = original;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(100));
            seen = read(&services).unwrap_or(original);
            if seen == target {
                break;
            }
        }

        let restore = write(&services, original);
        assert_eq!(
            seen, target,
            "set {target}% but the panel reports {seen}% (was {original}%)"
        );
        restore.expect("restoring the original brightness failed");
    }
}
