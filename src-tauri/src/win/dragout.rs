//! Dragging files *out* of the island, into Explorer or any other app.
//!
//! A webview cannot do this. HTML drag-and-drop has no way to hand a filesystem path
//! to a native drop target - the browser sandbox exists precisely to stop that - so
//! the drag has to start on the Win32 side, as an OLE drag, with the `CF_HDROP`
//! payload Explorer itself uses. The app receiving the drop has no idea the files
//! came from a web view.
//!
//! The data object is **not** hand-written. `IShellItemArray::BindToHandler` with
//! `BHID_DataObject` hands back the shell's own `IDataObject` for a set of items,
//! which is nine interface methods this does not have to implement or get wrong. Only
//! `IDropSource` is ours, and that is two methods: when to stop, and what cursor to
//! show.
//!
//! `DoDragDrop` blocks until the user drops or cancels, which is why the caller runs
//! this on a blocking thread rather than the UI one.

use windows::core::{implement, Interface, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{BOOL, HWND, POINT};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, OleUninitialize, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_LINK,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    SHCreateShellItemArrayFromIDLists, SHParseDisplayName, BHID_DataObject, IShellItemArray,
};

/// The drop source: the two decisions the system asks of whoever started a drag.
#[implement(IDropSource)]
struct DropSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for DropSource_Impl {
    /// Escape cancels, releasing the button drops, anything else carries on - the
    /// behaviour every Windows drag has.
    fn QueryContinueDrag(
        &self,
        escape_pressed: BOOL,
        key_state: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
    ) -> windows::core::HRESULT {
        use windows::Win32::System::SystemServices::MK_LBUTTON;

        if escape_pressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (key_state & MK_LBUTTON) == windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS(0)
        {
            return DRAGDROP_S_DROP;
        }
        windows::Win32::Foundation::S_OK
    }

    /// Returning DRAGDROP_S_USEDEFAULTCURSORS lets Windows draw its own drag
    /// cursors, so a drag from here looks like a drag from anywhere else.
    fn GiveFeedback(&self, _effect: DROPEFFECT) -> windows::core::HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// A PIDL that frees itself, because every early return here would otherwise leak one.
struct Pidl(*mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(self.0 as *const _)) };
        }
    }
}

fn data_object(paths: &[String]) -> Result<IDataObject> {
    unsafe {
        let mut pidls: Vec<Pidl> = Vec::with_capacity(paths.len());
        for p in paths {
            let wide = HSTRING::from(p.as_str());
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None)?;
            pidls.push(Pidl(pidl));
        }

        let raw: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| p.0 as *const _).collect();
        let array: IShellItemArray = SHCreateShellItemArrayFromIDLists(&raw)?;
        array.BindToHandler(None, &BHID_DataObject)
    }
}

/// Begin the drag. Returns true if something actually accepted a drop.
///
/// Blocks for as long as the user holds the drag.
pub fn start(paths: &[String]) -> bool {
    let existing: Vec<String> = paths
        .iter()
        .filter(|p| std::path::Path::new(p).exists())
        .cloned()
        .collect();
    if existing.is_empty() {
        return false;
    }

    unsafe {
        // OLE, not just COM: DoDragDrop needs the OLE layer on this thread. On the
        // main thread COM is already up, so this returns S_FALSE and only bumps the
        // reference count - which the matching OleUninitialize below gives back.
        let init = OleInitialize(None);
        if init.is_err() {
            crate::logln!("drag out: OleInitialize failed ({init:?})");
            return false;
        }

        let result = (|| -> Result<DROPEFFECT> {
            let data = data_object(&existing)?;
            let source: IDropSource = DropSource.into();
            let mut effect = DROPEFFECT::default();
            // Copy or link - never move. A shelf hands out references; dragging one
            // off must not take the original away from where it lives.
            let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
            Ok(effect)
        })();

        OleUninitialize();

        match result {
            Ok(effect) => {
                let dropped = effect != DROPEFFECT::default();
                crate::logln!("drag out: {} file(s), accepted={dropped}", existing.len());
                dropped
            }
            Err(e) => {
                crate::logln!("drag out failed: {e}");
                false
            }
        }
    }
}

/// Where the pointer is, for the frontend to decide a drag has left the island.
pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Unused placeholder kept out of the public surface.
#[allow(dead_code)]
fn _unused(_: HWND) {}
