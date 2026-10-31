//! Persisted settings, stored as JSON in `%APPDATA%\macui\settings.json`.
//!
//! Deleting that one file resets the app to defaults and affects nothing else on the
//! machine — no registry, no shared state. Writes go through a temp file + rename so a
//! crash mid-write cannot leave a truncated file that stops the dock loading.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Pinned entries, in the order the user dragged them into.
    pub pinned: Vec<PinnedEntry>,
    /// Set once the Windows taskbar pins have been imported, so a user who unpins
    /// everything does not get them all back on the next launch.
    pub pins_imported: bool,

    pub dock_enabled: bool,
    pub topbar_enabled: bool,
    pub topbar_auto_hide: bool,
    pub dock_always_visible: bool,
    pub dock_icon_size: u32,
    pub dock_magnification: bool,
    /// "circle" | "squircle" | "rounded"
    pub icon_shape: String,
    /// "auto" | "light" | "dark"
    pub theme: String,
    /// Multiplier on every animation duration. 1.0 = the designed timings.
    pub animation_speed: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedEntry {
    pub id: String,
    pub name: String,
    /// Shell parsing name: a `.lnk`, an `.exe`, or `shell:AppsFolder\<AUMID>`.
    pub parsing_name: String,
    /// Resolved target exe, used to match this pin against a running process.
    #[serde(default)]
    pub target: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pinned: Vec::new(),
            pins_imported: false,
            dock_enabled: true,
            topbar_enabled: true,
            topbar_auto_hide: false,
            dock_always_visible: false,
            dock_icon_size: 44,
            dock_magnification: true,
            icon_shape: "circle".into(),
            theme: "auto".into(),
            animation_speed: 1.0,
        }
    }
}

pub fn settings_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("macui").join("settings.json")
}

static CACHE: Mutex<Option<Settings>> = Mutex::new(None);

pub fn load() -> Settings {
    if let Some(s) = CACHE.lock().unwrap().clone() {
        return s;
    }
    let path = settings_path();
    let loaded = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| match serde_json::from_str::<Settings>(&t) {
            Ok(s) => Some(s),
            Err(e) => {
                // A corrupt settings file must not brick the dock; fall back to
                // defaults and say so, rather than failing to start.
                crate::logln!("settings.json unreadable ({e}); using defaults");
                None
            }
        })
        .unwrap_or_default();
    *CACHE.lock().unwrap() = Some(loaded.clone());
    loaded
}

pub fn save(s: &Settings) -> std::io::Result<()> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(s)?)?;
    // Atomic swap: readers see either the old file or the new one, never a partial.
    std::fs::rename(&tmp, &path)?;
    *CACHE.lock().unwrap() = Some(s.clone());
    Ok(())
}

/// Read-modify-write under one lock.
pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = load();
    f(&mut s);
    if let Err(e) = save(&s) {
        crate::logln!("failed to save settings: {e}");
    }
    s
}
