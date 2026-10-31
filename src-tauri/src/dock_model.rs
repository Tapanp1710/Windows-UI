//! Assembles what the dock shows: Start, pinned apps, running apps, Recycle Bin.
//!
//! The ordering and grouping rules are the taskbar's, because the dock is supposed to
//! *be* the taskbar visually and behave identically:
//!
//!   1. Start button (drawn by the frontend)
//!   2. Pinned apps, in the user's saved order. A pinned app that is also running
//!      shows the running indicator on the SAME button rather than appearing twice.
//!   3. Running apps that are not pinned, in window Z-order.
//!   4. A divider, then the Recycle Bin.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::settings;
use crate::win::{apps, icons, pins};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockItem {
    pub id: String,
    /// "start" | "app" | "recycle"
    pub kind: String,
    pub name: String,
    pub icon: Option<String>,
    pub running: bool,
    pub window_count: usize,
    pub attention: bool,
    pub progress: Option<f64>,
    pub progress_state: String,
    pub pinned: bool,
    /// Window handles behind this button, most recently used first.
    pub hwnds: Vec<isize>,
    /// What to launch when the button is clicked and nothing is running.
    pub parsing_name: Option<String>,
}

impl DockItem {
    fn blank(id: &str, kind: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            name: name.into(),
            icon: None,
            running: false,
            window_count: 0,
            attention: false,
            progress: None,
            progress_state: "none".into(),
            pinned: false,
            hwnds: Vec::new(),
            parsing_name: None,
        }
    }
}

/// Icon extraction goes through COM and costs a few milliseconds per app. The dock
/// refreshes roughly once a second, so caching by parsing name keeps that off the hot
/// path entirely — without it the dock would re-decode every icon on every tick.
static ICON_CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn cached_icon(parsing_name: &str) -> Option<String> {
    // These commands are async now, so this can land on a pool thread with no
    // apartment, and shell icon extraction needs one. It must be **multi-threaded**:
    // an STA thread is required to pump messages, and a tokio worker never does, so
    // marking one STA deadlocks the next COM call that needs marshalling. That is
    // exactly what hung the app the moment the launcher opened. MTA has no such
    // requirement. Only reached on a cache miss.
    unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut guard = ICON_CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(hit) = cache.get(parsing_name) {
        return hit.clone();
    }
    let icon = icons::icon_data_url(parsing_name);
    cache.insert(parsing_name.to_string(), icon.clone());
    icon
}

/// Drop cached icons, e.g. after the user changes icon shape or a pin is replaced.
pub fn clear_icon_cache() {
    *ICON_CACHE.lock().unwrap() = None;
}

/// Import the Windows taskbar pins once, on first run.
fn ensure_pins_imported() -> settings::Settings {
    let s = settings::load();
    if s.pins_imported {
        return s;
    }

    let imported: Vec<settings::PinnedEntry> = pins::read_taskbar_pins()
        .into_iter()
        .map(|p| {
            let target = pins::resolve_lnk_target(std::path::Path::new(&p.parsing_name))
                .map(|t| t.to_string_lossy().to_lowercase());
            settings::PinnedEntry {
                id: p.id,
                name: p.name,
                parsing_name: p.parsing_name,
                target,
            }
        })
        .collect();

    crate::logln!("importing {} Windows taskbar pins", imported.len());
    settings::update(|s| {
        s.pinned = imported;
        s.pins_imported = true;
    })
}

/// Build the current dock contents.
pub fn build() -> Vec<DockItem> {
    let settings = ensure_pins_imported();
    let groups = apps::list_app_groups();

    // Index running apps by lowercased exe path so pins can claim them.
    let mut by_exe: HashMap<String, apps::AppGroup> = HashMap::new();
    for g in groups {
        if let Some(p) = &g.exe_path {
            by_exe.insert(p.to_string_lossy().to_lowercase(), g);
        } else {
            by_exe.insert(g.key.clone(), g);
        }
    }

    let mut items = vec![DockItem::blank("start", "start", "Start")];

    // --- pinned, in the user's order --------------------------------------
    for pin in &settings.pinned {
        let mut item = DockItem::blank(&pin.id, "app", &pin.name);
        item.pinned = true;
        item.icon = cached_icon(&pin.parsing_name);
        item.parsing_name = Some(pin.parsing_name.clone());

        if let Some(target) = &pin.target {
            if let Some(group) = by_exe.remove(target) {
                item.running = true;
                item.window_count = group.windows.len();
                item.hwnds = group.windows.iter().map(|w| w.hwnd).collect();
            }
        }
        items.push(item);
    }

    // --- running but not pinned -------------------------------------------
    let mut running: Vec<apps::AppGroup> = by_exe.into_values().collect();
    // Stable, predictable order; Z-order within a group is preserved already.
    running.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    for group in running {
        // Never show ourselves in our own dock.
        if group
            .exe_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().eq_ignore_ascii_case("macui.exe"))
            .unwrap_or(false)
        {
            continue;
        }

        let mut item = DockItem::blank(&format!("run:{}", group.key), "app", &group.name);
        item.running = true;
        item.window_count = group.windows.len();
        item.hwnds = group.windows.iter().map(|w| w.hwnd).collect();
        if let Some(p) = &group.exe_path {
            let parsing = p.to_string_lossy().to_string();
            item.icon = cached_icon(&parsing);
            item.parsing_name = Some(parsing);
        }
        items.push(item);
    }

    // --- recycle bin -------------------------------------------------------
    let mut bin = DockItem::blank("recycle", "recycle", "Recycle Bin");
    bin.parsing_name = Some(pins::RECYCLE_BIN.to_string());
    bin.icon = cached_icon(pins::RECYCLE_BIN);
    // Name reflects state, the way Explorer's tooltip does.
    if !pins::recycle_bin_is_empty() {
        bin.name = "Recycle Bin (not empty)".into();
    }
    items.push(bin);

    items
}
