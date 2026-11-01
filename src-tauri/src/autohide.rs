//! Reveal/conceal logic for the dock.
//!
//! A background thread samples the cursor position and decides whether the dock
//! should be showing, emitting `dock:reveal` / `dock:conceal` to the frontend, which
//! runs the actual slide as a CSS transform.
//!
//! Why polling rather than a mouse hook: a `WH_MOUSE_LL` hook sits in the input path
//! of every mouse message on the system, and a slow or wedged hook degrades input for
//! the whole machine. The core rule for this project is that Windows must keep
//! behaving exactly as it does normally, so the cheap, inert option wins. Sampling
//! `GetCursorPos` ~20 times a second costs a couple of microseconds per sample and
//! cannot affect anyone else's input.
//!
//! Why this thread never touches a `WebviewWindow`: calling `outer_position()` or
//! `outer_size()` off the main thread dispatches onto the event loop and blocks until
//! it is serviced. Once the dock started doing real per-second IPC work, that
//! contention was enough to wedge this thread outright, and the dock simply stopped
//! hiding. The window rect is therefore published into `DOCK_RECT` by the main thread
//! whenever the dock moves or resizes, and read here as plain data.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

/// How close to the bottom edge the pointer must get to summon the dock.
/// Windows uses a 1px trigger for an auto-hidden taskbar; 2px is the same gesture
/// with a little tolerance for high-DPI rounding.
const TRIGGER_PX: i32 = 2;

/// Delay before hiding once the pointer leaves, per the brief.
const CONCEAL_DELAY: Duration = Duration::from_millis(400);

/// Sampling intervals.
///
/// 20ms even while hidden: the bars have to be there the moment the pointer reaches
/// the edge, and a 60ms poll put up to a tenth of a second in front of a 150ms
/// animation, which reads as hesitation. `GetCursorPos` costs a couple of
/// microseconds, so fifty of them a second is nothing - measured idle CPU is
/// unchanged.
const POLL_IDLE: Duration = Duration::from_millis(20);
const POLL_ACTIVE: Duration = Duration::from_millis(20);

/// Set from settings; when true the dock never auto-hides.
pub static ALWAYS_VISIBLE: AtomicBool = AtomicBool::new(false);

/// The sampler's current reveal state.
///
/// Exposed so the frontend can *ask* on startup instead of relying on catching an
/// event: the sampler emits only on a change, and anything emitted before the webview
/// has attached its listener is lost for good. That left the dock stuck concealed
/// whenever the pointer happened to already be at the bottom edge at launch.
pub static REVEALED: AtomicBool = AtomicBool::new(false);

/// The same, for the top bar.
pub static TOPBAR_REVEALED: AtomicBool = AtomicBool::new(false);
/// Height of the top bar window in physical pixels, published by the main thread.
/// While the bar is showing, the pointer anywhere inside this strip keeps it up.
pub static TOPBAR_HEIGHT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// Set from settings; when false the top bar is simply always there.
pub static TOPBAR_AUTO_HIDE: AtomicBool = AtomicBool::new(true);
/// True while one of the bar's panels is open. The bar sliding away from under an
/// open flyout looks like a bug, so it is held down until the panel closes.
pub static TOPBAR_PINNED: AtomicBool = AtomicBool::new(false);

/// Dock window rect in physical pixels: (left, top, right, bottom).
/// Published by the main thread, read by the sampler.
pub static DOCK_RECT: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);

pub fn publish_rect(left: i32, top: i32, width: i32, height: i32) {
    *DOCK_RECT.lock().unwrap() = Some((left, top, left + width, top + height));
}

fn cursor_over_dock(cx: i32, cy: i32) -> bool {
    match *DOCK_RECT.lock().unwrap() {
        Some((l, t, r, b)) => cx >= l && cx < r && cy >= t && cy < b,
        None => false,
    }
}

pub fn start(app: AppHandle) -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let stop = running.clone();

    std::thread::spawn(move || {
        let mut leave_at: Option<Instant> = None;
        let mut topbar_leave_at: Option<Instant> = None;
        let mut ticks: u32 = 0;

        // Start concealed: the dock should be out of the way until summoned. The
        // frontend may not have its listener attached yet, so it also starts in the
        // concealed CSS state and this is only a confirmation.
        let _ = app.emit("dock:conceal", ());
        let mut revealed = false;
        REVEALED.store(false, Ordering::SeqCst);
        // The top bar starts concealed too, and for the same reason: the frontend
        // starts in the concealed CSS state and the sampler only emits on a change.
        let _ = app.emit("topbar:conceal", ());
        TOPBAR_REVEALED.store(false, Ordering::SeqCst);
        crate::logln!("autohide sampler started");

        let (_, screen_h) = crate::win::taskbar::screen_size();

        while stop.load(Ordering::Relaxed) {
            let always = ALWAYS_VISIBLE.load(Ordering::Relaxed);
            let (cx, cy) = crate::win::cursor_pos();

            let at_edge = cy >= screen_h - TRIGGER_PX;
            // Stay up while the pointer is actually on the dock, so it does not
            // slide away from under the cursor mid-click.
            let want = always || at_edge || (revealed && cursor_over_dock(cx, cy));

            if want {
                leave_at = None;
                if !revealed {
                    revealed = true;
                    REVEALED.store(true, Ordering::SeqCst);
                    let _ = app.emit("dock:reveal", ());
                }
            } else if revealed {
                match leave_at {
                    None => leave_at = Some(Instant::now() + CONCEAL_DELAY),
                    Some(t) if Instant::now() >= t => {
                        revealed = false;
                        REVEALED.store(false, Ordering::SeqCst);
                        leave_at = None;
                        let _ = app.emit("dock:conceal", ());
                    }
                    Some(_) => {}
                }
            }

            // Cheap periodic re-assert: a display change or a shell event can reset
            // the work area, and a silently-written reservation has no notification
            // to hook, so it is checked here rather than trusted once.
            ticks = ticks.wrapping_add(1);

            // --- dismiss an open panel on a click outside it --------------------
            // The panel does not take focus, so "you clicked elsewhere" has to be
            // observed rather than delivered. GetAsyncKeyState is a read of the
            // current button state; it installs nothing and sees nothing but the
            // buttons' own up/down flags.
            // Copied out on its own line so the guard drops here. Inside an `if let`
            // scrutinee it would live through the body, and `close_panel` takes the
            // same lock: this thread deadlocked on itself, and the main thread hung
            // behind it in the panel's `Destroyed` handler (D-056).
            let panel = *crate::PANEL_RECT.lock().unwrap();
            if let Some((l, t, r, b)) = panel {
                let outside = cx < l || cx >= r || cy < t || cy >= b;
                if outside && crate::win::mouse_button_down() {
                    crate::logln!("panel dismissed: clicked outside");
                    crate::close_panel(&app);
                }
            }

            // --- top bar ------------------------------------------------------
            // Same gesture as the dock, mirrored: touch the top edge and it comes
            // down; move away and it goes back up. While it is down, the pointer
            // anywhere in its strip keeps it there, so it cannot slide away from
            // under a click.
            {
                let auto = TOPBAR_AUTO_HIDE.load(Ordering::Relaxed);
                let shown = TOPBAR_REVEALED.load(Ordering::Relaxed);
                let want = if !auto || TOPBAR_PINNED.load(Ordering::Relaxed) {
                    true
                } else {
                    cy <= TRIGGER_PX || (shown && cy < TOPBAR_HEIGHT.load(Ordering::Relaxed))
                };

                if want {
                    topbar_leave_at = None;
                    if !shown {
                        TOPBAR_REVEALED.store(true, Ordering::SeqCst);
                        let _ = app.emit("topbar:reveal", ());
                    }
                } else if shown {
                    match topbar_leave_at {
                        None => topbar_leave_at = Some(Instant::now() + CONCEAL_DELAY),
                        Some(t) if Instant::now() >= t => {
                            TOPBAR_REVEALED.store(false, Ordering::SeqCst);
                            topbar_leave_at = None;
                            let _ = app.emit("topbar:conceal", ());
                        }
                        Some(_) => {}
                    }
                }
            }

            // Remember what the user was working in, for the widget layer to hand
            // focus back to. Sampled here rather than hooked, for the same reason
            // the cursor is: a shell hook would sit in everyone's input path.
            if let Some(h) = crate::win::apps::foreground_window() {
                let raw = h.0 as isize;
                if !crate::win::apps::is_our_window(raw) {
                    crate::LAST_FOREIGN_FOREGROUND.store(raw, Ordering::SeqCst);
                }
            }

            // Keep the widget layer parked just above the desktop. The shell re-sorts
            // the z-order whenever a window is activated or Win+D is pressed, and
            // there is no notification to hook, so it is re-asserted on the sampler
            // that is already running rather than on a thread of its own.
            if ticks % 20 == 0 {
                let hwnd = crate::WIDGETS_HWND.load(Ordering::SeqCst);
                if hwnd != 0 {
                    crate::win::place_above_desktop(hwnd);
                }
            }

            // The Windows taskbar can come back without us doing anything: opening
            // the shell's "Open with" dialog did exactly that during PHASE 5 testing,
            // leaving a visible taskbar sitting under our dock. Hiding it once at
            // startup is not enough, so it is re-asserted here - except while the
            // tray overflow is deliberately showing it.
            if ticks % 20 == 10
                && !crate::TASKBAR_SHOW_ALLOWED.load(Ordering::SeqCst)
                && crate::win::taskbar::is_taskbar_visible()
            {
                crate::logln!("the Windows taskbar reappeared; hiding it again");
                crate::win::taskbar::hide_taskbar();
            }

            if ticks % 40 == 0 {
                let want = crate::RESERVED_TOP.load(Ordering::SeqCst);
                if want > 0 && crate::win::taskbar::work_area().top != want {
                    crate::win::taskbar::set_reserved(want, 0);
                }
            }

            std::thread::sleep(if revealed { POLL_ACTIVE } else { POLL_IDLE });
        }
        crate::logln!("autohide sampler stopped");
    });

    running
}
