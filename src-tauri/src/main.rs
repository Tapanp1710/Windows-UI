// No console window on a release build; a shell replacement flashing a console on
// every login would be unacceptable.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autohide;
mod dock_model;
#[macro_use]
mod log;
mod settings;
mod startup;
mod wallpaper;
mod watchdog;
mod win;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{Emitter, Manager, WindowEvent};

/// Taskbar state as it was before we touched anything, so the restore puts back the
/// user's own setting rather than a guessed default.
static ORIGINAL_APPBAR_STATE: Mutex<Option<u32>> = Mutex::new(None);
/// Guards against running the restore twice (tray Quit + window close + process exit
/// can all fire).
static RESTORED: AtomicBool = AtomicBool::new(false);
/// The watchdog child, killed on a clean shutdown so it does not fire a redundant
/// restore after we have already done one.
static WATCHDOG: Mutex<Option<std::process::Child>> = Mutex::new(None);
/// Pixels reserved at the top for the top bar, re-asserted periodically because
/// other shell events can reset the work area behind our back.
pub static RESERVED_TOP: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// Kept alive for the life of the app; cleared on shutdown to stop the sampler.
static AUTOHIDE_RUNNING: Mutex<Option<std::sync::Arc<AtomicBool>>> = Mutex::new(None);

/// Put Windows back exactly as we found it. Idempotent and safe to call from any
/// shutdown path.
fn restore_windows_state() {
    if RESTORED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(flag) = AUTOHIDE_RUNNING.lock().unwrap().take() {
        flag.store(false, Ordering::SeqCst);
    }
    RESERVED_TOP.store(0, Ordering::SeqCst);
    let state = *ORIGINAL_APPBAR_STATE.lock().unwrap();
    win::taskbar::full_restore(state);

    if let Some(mut child) = WATCHDOG.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn main() {
    // --- watchdog mode -------------------------------------------------------
    // Must be the very first thing: a watchdog process must not start a UI, a tray
    // icon, or a second webview.
    if let Some((pid, state)) = watchdog::parse_args(std::env::args()) {
        watchdog::run(pid, state);
        return;
    }

    // --- normal startup ------------------------------------------------------
    log::init("main");
    win::init_dpi_awareness();
    // Shell interfaces (icon extraction, ShellExecute) need COM on this thread.
    win::init_com();

    let original_state = win::taskbar::appbar_state();
    *ORIGINAL_APPBAR_STATE.lock().unwrap() = Some(original_state);
    logln!(
        "taskbar state at startup: {}",
        win::taskbar::describe_state(original_state)
    );

    // Start the watchdog BEFORE hiding anything, so there is never a window of time
    // where the taskbar is hidden and nothing is watching us.
    match watchdog::spawn(Some(original_state)) {
        Ok(child) => {
            logln!("watchdog started (pid {})", child.id());
            *WATCHDOG.lock().unwrap() = Some(child);
        }
        Err(e) => {
            // Without a watchdog we would risk stranding the user with no taskbar if
            // we were killed. Refuse to hide it rather than take that risk.
            logln!("WARNING: watchdog failed to start ({e}); leaving the taskbar visible");
        }
    }

    let watchdog_ok = WATCHDOG.lock().unwrap().is_some();
    if watchdog_ok {
        win::taskbar::hide_taskbar();
        // The dock auto-hides, so it reserves nothing - exactly as an auto-hidden
        // Windows taskbar behaves. The top bar is permanently visible and reserves
        // its strip, but that happens in setup() once its window handle exists.
        win::taskbar::reset_work_area();
    }

    // Make sure a panic still restores the taskbar. With `panic = "abort"` the hook
    // runs before the abort, so this is our last chance.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_windows_state();
        default_hook(info);
    }));

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            cmd_screen_metrics,
            cmd_restore_now,
            cmd_resize_dock,
            cmd_system_prefs,
            cmd_dock_items,
            cmd_dock_activate,
            cmd_dock_launch,
            cmd_dock_close_window,
            cmd_dock_set_pinned_order,
            cmd_dock_unpin,
            cmd_dock_pin,
            cmd_window_titles,
            cmd_open_start_menu,
            cmd_log,
            cmd_app_quit,
            cmd_app_restart,
            cmd_dock_revealed,
            cmd_status,
            cmd_set_topbar_regions,
            cmd_open_quick_settings,
            cmd_open_notifications,
            cmd_open_tray_overflow,
            cmd_set_volume,
            cmd_focus_foreground,
            cmd_open_settings,
            cmd_close_settings,
            cmd_get_config,
            cmd_patch_settings,
            cmd_set_start_at_login,
            cmd_set_wallpaper,
            cmd_return_to_normal,
        ])
        .setup(|app| {
            if let Err(e) = build_tray(app.handle()) {
                // A missing tray icon is not fatal, but it removes the only
                // discoverable way to quit, so say so loudly in the log.
                logln!("WARNING: could not create the tray icon ({e}); quit via restore-windows.ps1");
            }
            // Prove the embedded frontend is actually reachable. A blank window with
            // no error is otherwise indistinguishable from a CSS bug, and these two
            // lines turn that into a one-line answer in the log.
            // One line that distinguishes "my CSS is wrong" from "the frontend was
            // never embedded" - the exact confusion that cost a long detour here.
            let assets: Vec<String> = app
                .asset_resolver()
                .iter()
                .map(|(k, _)| k.to_string())
                .collect();
            logln!(
                "embedded assets: {} (dock.html present: {})",
                assets.len(),
                assets.iter().any(|k| k.ends_with("dock.html"))
            );
            position_dock(app.handle())?;
            position_topbar(app.handle())?;
            apply_settings(app.handle(), true);

            // With both bars switched off there would be no way left to reach the
            // settings - the tray icon lives inside the taskbar we hide. Open the
            // settings window instead of starting up invisible and unreachable.
            let s = settings::load();
            if !s.dock_enabled && !s.topbar_enabled {
                logln!("both bars are disabled; opening settings so the UI stays reachable");
                if let Err(e) = open_settings(app.handle()) {
                    logln!("could not open the settings window: {e}");
                }
            }
            AUTOHIDE_RUNNING
                .lock()
                .unwrap()
                .replace(autohide::start(app.handle().clone()));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                if window.label() == "dock" {
                    restore_windows_state();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error building the macui app")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                restore_windows_state();
            }
        });

    // Belt and braces for any exit path that skips the RunEvent above.
    restore_windows_state();
}

/// Screen geometry the frontend needs for layout, in physical pixels plus the scale
/// factor so the frontend can reason in CSS pixels.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenMetrics {
    width: i32,
    height: i32,
    work_bottom: i32,
    scale: f64,
}

#[tauri::command]
fn cmd_screen_metrics(window: tauri::Window) -> ScreenMetrics {
    let (w, h) = win::taskbar::screen_size();
    let wa = win::taskbar::work_area();
    ScreenMetrics {
        width: w,
        height: h,
        work_bottom: wa.bottom,
        scale: window.scale_factor().unwrap_or(1.0),
    }
}

/// Exposed so the settings UI's "return to normal Windows" button and the tray's Quit
/// both route through exactly the same code path as a crash restore.
#[tauri::command]
fn cmd_restore_now() {
    restore_windows_state();
}

/// Everything the top bar shows, in one round trip.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusPayload {
    battery: win::status::Battery,
    volume: win::status::Volume,
    network: win::status::Network,
    focused: win::status::FocusedApp,
    bluetooth: BluetoothStatus,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BluetoothStatus {
    present: bool,
    on: bool,
}

#[tauri::command]
fn cmd_status() -> StatusPayload {
    let (present, on) = win::bluetooth();
    StatusPayload {
        battery: win::status::battery(),
        volume: win::status::volume(),
        network: win::status::network(),
        focused: win::status::focused_app(),
        bluetooth: BluetoothStatus { present, on },
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegionRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// Limit the top bar window's clickable area to its three pills.
#[tauri::command]
fn cmd_set_topbar_regions(window: tauri::Window, rects: Vec<RegionRect>) -> tauri::Result<()> {
    let scale = window.scale_factor().unwrap_or(1.0);
    // The frontend measures in CSS pixels; SetWindowRgn works in physical ones.
    let phys: Vec<(i32, i32, i32, i32)> = rects
        .iter()
        .map(|r| {
            (
                (r.x as f64 * scale).floor() as i32,
                (r.y as f64 * scale).floor() as i32,
                (r.w as f64 * scale).ceil() as i32,
                (r.h as f64 * scale).ceil() as i32,
            )
        })
        .collect();
    let hwnd = window.hwnd()?.0 as isize;
    win::set_window_region(hwnd, &phys);
    Ok(())
}

#[tauri::command]
fn cmd_open_quick_settings() {
    win::status::open_quick_settings();
}

#[tauri::command]
fn cmd_open_notifications() {
    win::status::open_notification_centre();
}

/// Open the real tray overflow so other apps' tray icons stay reachable.
///
/// The overflow button lives inside the taskbar we hide, so the taskbar is shown just
/// long enough for the click to land, then hidden again.
#[tauri::command]
fn cmd_open_tray_overflow() {
    win::taskbar::restore_taskbar(None);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let clicked = win::open_tray_overflow();
    if !clicked {
        // Windows 11 moved the overflow into a XAML host with no clickable child
        // window; Quick Settings is the closest real panel.
        win::status::open_quick_settings();
    }
    // Give the flyout a moment to take focus before the taskbar goes away again,
    // otherwise hiding the parent dismisses it.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(2500));
        win::taskbar::hide_taskbar();
    });
}

#[tauri::command]
fn cmd_set_volume(level: u8) -> bool {
    win::status::set_volume(level)
}

/// Re-assert focus on the foreground window. Harmless no-op when already focused.
#[tauri::command]
fn cmd_focus_foreground() {
    if let Some(h) = win::apps::foreground_window() {
        win::apps::activate(h.0 as isize);
    }
}

/// Height of the top bar strip in CSS pixels, mirrored from tauri.conf.json.
const TOPBAR_CSS_HEIGHT: f64 = 42.0;

/// Reserve screen space for whichever bars are permanently visible.
///
/// Registers a real appbar rather than writing the work area directly: the shell
/// recomputes the work area from its registered appbars, so a direct
/// `SPI_SETWORKAREA` write is silently discarded (verified - it did nothing).
fn apply_work_area(app: &tauri::AppHandle) {
    let settings = settings::load();
    let Some(bar) = app.get_webview_window("topbar") else {
        return;
    };

    if !settings.topbar_enabled || settings.topbar_auto_hide {
        // Nothing to reserve. `appbar::release()` is a no-op unless an appbar was
        // ever registered (see D-010); resetting the work area is what actually
        // hands the strip back, and it must happen or a disabled top bar would
        // leave a 63px dead band at the top of every maximised window.
        win::appbar::release();
        RESERVED_TOP.store(0, Ordering::SeqCst);
        win::taskbar::reset_work_area();
        return;
    }

    let scale = bar.scale_factor().unwrap_or(1.0);
    let top = (TOPBAR_CSS_HEIGHT * scale).round() as i32;
    // The dock auto-hides, so like an auto-hidden taskbar it reserves nothing.
    win::taskbar::set_reserved(top, 0);
    RESERVED_TOP.store(top, Ordering::SeqCst);
    logln!("reserved {top}px at the top of the work area for the top bar");
}

/// Full screen width, flush with the top edge; the CSS supplies the 6px inset.
fn position_topbar(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(bar) = app.get_webview_window("topbar") else {
        return Ok(());
    };
    let (sw, _) = win::taskbar::screen_size();
    let scale = bar.scale_factor().unwrap_or(1.0);
    let h = (56.0 * scale).round() as i32;
    bar.set_size(tauri::PhysicalSize::new(sw as u32, h as u32))?;
    bar.set_position(tauri::PhysicalPosition::new(0, 0))?;
    Ok(())
}

/// System-tray icon: the supported way to quit.
///
/// Without this the only way to stop a shell replacement is Task Manager, which kills
/// the whole process tree — watchdog included — and that is exactly the one case where
/// the taskbar does not come back on its own. Quit here always restores Windows first.
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;

    let settings_item =
        MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "Restart UI", true, None::<&str>)?;
    let normal = MenuItem::with_id(
        app,
        "normal",
        "Return to normal Windows",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&settings_item, &restart, &normal, &sep, &quit])?;

    TrayIconBuilder::with_id("macui-tray")
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("the app icon is embedded at build time, so this cannot be absent"),
        )
        .tooltip("macui — macOS-inspired UI layer")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => {
                if let Err(e) = open_settings(app) {
                    logln!("could not open the settings window: {e}");
                }
            }
            "quit" => {
                restore_windows_state();
                app.exit(0);
            }
            "normal" => {
                return_to_normal(app);
            }
            "restart" => {
                restore_windows_state();
                app.restart();
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Settings the frontend mirrors onto `<html>` so CSS can react to them.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemPrefs {
    animations: bool,
    icon_shape: String,
    theme: String,
    animation_speed: f64,
    magnification: bool,
    always_visible: bool,
    icon_size: u32,
}

#[tauri::command]
fn cmd_system_prefs() -> SystemPrefs {
    let s = settings::load();
    SystemPrefs {
        // Respect Windows' own animation setting as well as ours.
        animations: win::animations_enabled(),
        icon_shape: s.icon_shape,
        theme: s.theme,
        animation_speed: s.animation_speed,
        magnification: s.dock_magnification,
        always_visible: s.dock_always_visible,
        icon_size: s.dock_icon_size,
    }
}

/// Everything the dock draws. Called on a timer by the frontend.
#[tauri::command]
fn cmd_dock_items() -> Vec<dock_model::DockItem> {
    dock_model::build()
}

/// A left-click on a dock button, with the taskbar's exact semantics:
///   - nothing running        -> launch it
///   - running, not focused   -> bring it to the front
///   - running and focused    -> minimise it
///
/// With several windows the frontend asks for a chooser instead, so this only ever
/// receives a single concrete hwnd in that case.
#[tauri::command]
fn cmd_dock_activate(hwnd: isize) {
    let focused = win::apps::foreground_window().map(|h| h.0 as isize);
    if focused == Some(hwnd) {
        win::apps::minimize(hwnd);
    } else {
        win::apps::activate(hwnd);
    }
}

/// Launch (or start a new instance of) whatever the button points at.
#[tauri::command]
fn cmd_dock_launch(parsing_name: String) -> bool {
    win::pins::launch(&parsing_name)
}

#[tauri::command]
fn cmd_dock_close_window(hwnd: isize) {
    win::apps::close_window(hwnd);
}

/// Persist the order after a drag-to-reorder.
#[tauri::command]
fn cmd_dock_set_pinned_order(ids: Vec<String>) {
    settings::update(|s| {
        let mut reordered = Vec::with_capacity(s.pinned.len());
        for id in &ids {
            if let Some(p) = s.pinned.iter().find(|p| &p.id == id) {
                reordered.push(p.clone());
            }
        }
        // Anything not named in `ids` keeps its place at the end rather than being
        // silently dropped.
        for p in &s.pinned {
            if !reordered.iter().any(|q| q.id == p.id) {
                reordered.push(p.clone());
            }
        }
        s.pinned = reordered;
    });
}

#[tauri::command]
fn cmd_dock_unpin(id: String) {
    settings::update(|s| s.pinned.retain(|p| p.id != id));
}

#[tauri::command]
fn cmd_dock_pin(name: String, parsing_name: String) {
    let target = Some(parsing_name.to_lowercase());
    let id = format!("pin:{}", name.to_lowercase());
    settings::update(|s| {
        if s.pinned.iter().any(|p| p.id == id) {
            return;
        }
        s.pinned.push(settings::PinnedEntry {
            id: id.clone(),
            name: name.clone(),
            parsing_name: parsing_name.clone(),
            target: target.clone(),
        });
    });
}

/// Titles for the grouped-window chooser.
#[tauri::command]
fn cmd_window_titles(hwnds: Vec<isize>) -> Vec<String> {
    let all = win::apps::list_windows();
    hwnds
        .into_iter()
        .map(|h| {
            all.iter()
                .find(|w| w.hwnd == h)
                .map(|w| w.title.clone())
                .unwrap_or_default()
        })
        .collect()
}

/// Open the real Windows Start menu.
///
/// Until PHASE 3 ships the launcher, the Start button must still do the correct
/// Windows thing rather than nothing. This synthesises the same Ctrl+Esc that has
/// opened Start since Windows 95 — it is the documented, non-invasive way to ask the
/// shell to show it, and it does not remap or consume any key.
/// Lets the frontend write into the same log file as the backend. These windows have
/// no console the user can open, so without this a frontend failure is invisible.
#[tauri::command]
fn cmd_log(msg: String) {
    logln!("[ui] {msg}");
}

/// Current reveal state, so the dock can sync on startup rather than hoping it
/// caught the event.
#[tauri::command]
fn cmd_dock_revealed() -> bool {
    autohide::REVEALED.load(Ordering::SeqCst)
}

/// Quit, always putting Windows back first.
#[tauri::command]
fn cmd_app_quit(app: tauri::AppHandle) {
    restore_windows_state();
    app.exit(0);
}

#[tauri::command]
fn cmd_app_restart(app: tauri::AppHandle) {
    restore_windows_state();
    app.restart();
}

#[tauri::command]
fn cmd_open_start_menu() {
    win::press_ctrl_esc();
}

/// Resize the dock window to the island's measured size, then re-centre it.
///
/// The frontend measures in CSS pixels; Tauri's `LogicalSize` applies the monitor
/// scale factor for us (verified in STEP 0 that CSS px == logical px here).
#[tauri::command]
fn cmd_resize_dock(window: tauri::Window, css_width: f64, css_height: f64) -> tauri::Result<()> {
    window.set_size(tauri::LogicalSize::new(css_width, css_height))?;
    reposition(&window)?;
    Ok(())
}

/// The dock window's visible island already carries its own 12px margin inside the
/// shadow padding, so the window itself is flush with the bottom of the screen.
/// Centre it horizontally; the CSS does the rest.
fn reposition(window: &tauri::Window) -> tauri::Result<()> {
    let size = window.outer_size()?;
    let (sw, sh) = win::taskbar::screen_size();
    let x = (sw - size.width as i32) / 2;
    let y = sh - size.height as i32;
    window.set_position(tauri::PhysicalPosition::new(x, y))?;
    // Publish for the auto-hide sampler, which must never call back into the
    // window from its own thread (see autohide.rs).
    autohide::publish_rect(x, y, size.width as i32, size.height as i32);
    Ok(())
}

fn position_dock(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(dock) = app.get_webview_window("dock") {
        reposition(&dock.as_ref().window())?;
    }
    Ok(())
}

/* ------------------------------------------------------------------ PHASE 6
 * Settings: one on-demand window, the tray menu, the startup entry, and the
 * "return to normal Windows" button. Everything routes through the same
 * `settings.json` the dock and top bar already read, and through the same
 * `restore_windows_state()` the crash path uses.
 */

/// Open (or re-focus) the settings window.
///
/// It is built on demand rather than listed in `tauri.conf.json`, because every live
/// window is a WebView2 renderer process: creating it only when asked keeps it out of
/// the idle memory figure entirely.
fn open_settings(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }

    // Created VISIBLE, unlike the dock and the top bar.
    //
    // Those two are shown by their own page once it has painted, to avoid a flash.
    // Doing the same here was a bug: a window built hidden gets a WebView2 controller
    // with IsVisible=false sized 0x0, so if anything in the page fails before its
    // `show()` call the window is stranded invisible with no way to find out why.
    // This window is transparent and paints nothing until the page is ready, so
    // there is no flash to avoid in the first place.
    let w = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("macui settings")
    .inner_size(480.0, 660.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .center()
    .build()?;
    logln!("settings window created");
    // Focus is a courtesy, not a requirement: a failure here must not look like a
    // failure to open the window.
    if let Err(e) = w.set_focus() {
        logln!("settings window could not take focus: {e}");
    }
    Ok(())
}

/// Deliberately `async`.
///
/// A synchronous command runs on the main thread *inside* the WebView2 callback that
/// delivered the IPC message. Creating a second webview from there leaves the new
/// controller half-created: the window appears, the child WebView2 widget is 0x0 and
/// hidden, and nothing ever paints - with no error anywhere to explain it. Tauri
/// documents this: window creation belongs in an async command, which runs off the
/// main thread and dispatches the build properly.
#[tauri::command]
async fn cmd_open_settings(app: tauri::AppHandle) -> Result<(), String> {
    open_settings(&app).map_err(|e| e.to_string())
}

#[tauri::command]
fn cmd_close_settings(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.close();
    }
}

/// Everything the settings window needs to draw itself.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigPayload {
    settings: settings::Settings,
    /// Read from the filesystem, not from `settings.json`: the shortcut's presence in
    /// the Startup folder is the single source of truth, so deleting it by hand (or
    /// disabling it in Task Manager) is reflected here rather than contradicted.
    start_at_login: bool,
    /// Whether we have a recorded wallpaper to put back.
    wallpaper_changed: bool,
    settings_path: String,
    version: &'static str,
}

#[tauri::command]
fn cmd_get_config() -> ConfigPayload {
    logln!("settings window asked for the current configuration");
    let s = settings::load();
    ConfigPayload {
        wallpaper_changed: s.original_wallpaper.is_some(),
        start_at_login: startup::is_enabled(),
        settings_path: settings::settings_path().to_string_lossy().to_string(),
        version: env!("CARGO_PKG_VERSION"),
        settings: s,
    }
}

/// Merge a partial settings object into the stored one and apply it live.
///
/// A patch rather than a whole-object write: the settings window never sees `pinned`,
/// and sending the whole struct back would let a window that has been open since
/// before a drag-to-reorder silently undo it.
#[tauri::command]
fn cmd_patch_settings(app: tauri::AppHandle, patch: serde_json::Value) -> Result<(), String> {
    let incoming = patch.as_object().ok_or("the patch must be an object")?;
    let mut current = serde_json::to_value(settings::load()).map_err(|e| e.to_string())?;
    let map = current
        .as_object_mut()
        .ok_or("settings did not serialise to an object")?;
    for (k, v) in incoming {
        map.insert(k.clone(), v.clone());
    }
    let next: settings::Settings = serde_json::from_value(current).map_err(|e| e.to_string())?;
    settings::save(&next).map_err(|e| e.to_string())?;
    apply_settings(&app, false);
    Ok(())
}

/// The full "undo everything" path, shared by the tray item and the settings button.
///
/// This is deliberately more than Quit: quitting leaves a wallpaper the user chose
/// deliberately in place, whereas "return to normal Windows" means the machine looks
/// exactly as it did before any of this ran - the same promise `restore-windows.ps1`
/// makes.
fn return_to_normal(app: &tauri::AppHandle) {
    if settings::load().original_wallpaper.is_some() {
        wallpaper::set(app, "restore");
    }
    restore_windows_state();
    app.exit(0);
}

#[tauri::command]
fn cmd_return_to_normal(app: tauri::AppHandle) {
    return_to_normal(&app);
}

#[tauri::command]
fn cmd_set_start_at_login(enabled: bool) -> Result<bool, String> {
    startup::set(enabled).map_err(|e| e.to_string())?;
    // Report what is actually on disk now, not what was asked for.
    Ok(startup::is_enabled())
}

/// `which` is "light", "dark" or "restore".
#[tauri::command]
fn cmd_set_wallpaper(app: tauri::AppHandle, which: String) -> bool {
    wallpaper::set(&app, &which)
}

/// Push the current settings into the running UI.
///
/// `initial` is true during `setup()`, where the enabled windows must NOT be shown
/// from here: each page shows itself once it has painted, and showing it early is a
/// visible flash of an empty window.
fn apply_settings(app: &tauri::AppHandle, initial: bool) {
    let s = settings::load();

    // This is the only place the dock's "always visible" setting reaches the
    // sampler; before PHASE 6 it was stored and then never read.
    autohide::ALWAYS_VISIBLE.store(s.dock_always_visible, Ordering::SeqCst);

    for (label, enabled) in [("dock", s.dock_enabled), ("topbar", s.topbar_enabled)] {
        let Some(w) = app.get_webview_window(label) else {
            continue;
        };
        if enabled {
            if !initial {
                let _ = w.show();
            }
        } else {
            let _ = w.hide();
        }
    }

    apply_work_area(app);
    // The dock and the top bar both re-read `cmd_system_prefs` on this event, so
    // theme, icon shape and animation speed change without a restart.
    let _ = app.emit("dock:prefs", ());
}
