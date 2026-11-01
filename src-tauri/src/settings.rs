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
    /// The focused-app pill on the left of the top bar. Off by default: it is the
    /// one pill that repeats what the window in front of you already says.
    pub topbar_app_pill: bool,
    /// The centre pill behaves as a Dynamic Island: it expands on hover to show
    /// what is playing, and by itself for a moment when the volume changes, the
    /// charger goes in or out, or the track changes.
    pub island: bool,
    /// Things parked on the island: files, folders, anything dropped there. Kept
    /// across restarts, because a shelf you have to refill is not a shelf.
    pub shelf: Vec<ShelfItem>,

    /// Our Alt+Tab. Off hands the keystroke straight back to Windows: the hook stops
    /// swallowing Tab, so the native switcher appears exactly as it always did.
    pub switcher_enabled: bool,
    /// Our launcher on the Win key. Off means the Win key is never touched at all
    /// and Windows Start opens exactly as it always did.
    pub launcher_enabled: bool,
    pub dock_always_visible: bool,
    pub dock_icon_size: u32,
    pub dock_magnification: bool,
    /// "circle" | "squircle" | "rounded"
    pub icon_shape: String,
    /// "auto" | "light" | "dark"
    pub theme: String,
    /// Multiplier on every animation duration. 1.0 = the designed timings.
    pub animation_speed: f64,
    /// Hue (0-360) the glass is tinted with, or `None` for clear, colourless glass.
    /// Every surface - the dock, the launcher, the panels, the acrylic behind them -
    /// takes its colour from this one value. Chosen from the dock's tray.
    #[serde(default)]
    pub glass_hue: Option<f64>,

    /// Desktop widgets: master switch, then one entry per widget.
    pub widgets_enabled: bool,
    /// Keyed by widget id ("clock", "calendar", "weather", "media", "stats").
    /// Absent keys mean "not placed yet" - the frontend lays those out itself, so a
    /// new widget in a later version gets a sensible position without a migration.
    pub widgets: std::collections::BTreeMap<String, WidgetState>,

    /// Weather location. Latitude/longitude are what Open-Meteo actually needs; the
    /// name is only what we show and what the settings box searched for.
    pub weather_city: String,
    pub weather_lat: f64,
    pub weather_lon: f64,

    /// Whatever Windows had as its wallpaper before the user accepted ours, so the
    /// settings window and `restore-windows.ps1` can put it back. `None` means we
    /// have not changed it.
    #[serde(default)]
    pub original_wallpaper: Option<String>,
}

/// One thing parked on the island.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelfItem {
    /// Full path for a file or folder; the window handle as text for a window.
    pub id: String,
    pub name: String,
    /// "file", "folder" or "window".
    pub kind: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// One widget's switch and its position, in CSS pixels from the top left of the
/// widget layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetState {
    pub on: bool,
    pub x: f64,
    pub y: f64,
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
            // Off. Everything it carried - network, Bluetooth, sound, battery, the
            // clock and the shelf - now lives in the dock's own tray, so there is
            // one bar at the bottom rather than two on opposite edges. The top bar
            // is kept, and the switch in settings still brings it back, but having
            // both on means two clocks and two sets of the same controls.
            topbar_enabled: false,
            // On by default. A permanently visible bar has to reserve its strip,
            // which stops maximised windows using the whole screen; auto-hiding
            // gives every pixel back and the bar is one pointer-flick away.
            topbar_auto_hide: true,
            topbar_app_pill: false,
            island: true,
            shelf: Vec::new(),
            switcher_enabled: true,
            launcher_enabled: true,
            dock_always_visible: false,
            dock_icon_size: 44,
            dock_magnification: true,
            icon_shape: "circle".into(),
            theme: "auto".into(),
            animation_speed: 1.0,
            glass_hue: None,
            widgets_enabled: true,
            widgets: std::collections::BTreeMap::new(),
            // Logged in DECISIONS (D-017): the brief says to default to a city in
            // India and say which, so this is Bengaluru. Change it in settings.
            weather_city: "Bengaluru".into(),
            weather_lat: 12.9716,
            weather_lon: 77.5946,
            original_wallpaper: None,
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
