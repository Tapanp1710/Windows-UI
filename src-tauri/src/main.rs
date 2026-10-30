// No console window on a release build; a shell replacement flashing a console on
// every login would be unacceptable.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autohide;
mod dock_model;
#[macro_use]
mod log;
mod settings;
mod watchdog;
mod win;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{Manager, WindowEvent};

/// Taskbar state as it was before we touched anything, so the restore puts back the
/// user's own setting rather than a guessed default.
static ORIGINAL_APPBAR_STATE: Mutex<Option<u32>> = Mutex::new(None);
/// Guards against running the restore twice (tray Quit + window close + process exit
/// can all fire).
static RESTORED: AtomicBool = AtomicBool::new(false);
/// The watchdog child, killed on a clean shutdown so it does not fire a redundant
/// restore after we have already done one.
static WATCHDOG: Mutex<Option<std::process::Child>> = Mutex::new(None);
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
        // The dock auto-hides, so maximised windows should use the full screen
        // height, exactly as they do with an auto-hidden Windows taskbar.
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
        ])
        .setup(|app| {
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
