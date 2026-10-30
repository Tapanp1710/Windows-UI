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

/// Sampling intervals. Slower while hidden, since nothing is on screen to feel laggy.
const POLL_IDLE: Duration = Duration::from_millis(60);
const POLL_ACTIVE: Duration = Duration::from_millis(40);

/// Set from settings; when true the dock never auto-hides.
pub static ALWAYS_VISIBLE: AtomicBool = AtomicBool::new(false);

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

        // Start concealed: the dock should be out of the way until summoned. The
        // frontend may not have its listener attached yet, so it also starts in the
        // concealed CSS state and this is only a confirmation.
        let _ = app.emit("dock:conceal", ());
        let mut revealed = false;
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
                    let _ = app.emit("dock:reveal", ());
                }
            } else if revealed {
                match leave_at {
                    None => leave_at = Some(Instant::now() + CONCEAL_DELAY),
                    Some(t) if Instant::now() >= t => {
                        revealed = false;
                        leave_at = None;
                        let _ = app.emit("dock:conceal", ());
                    }
                    Some(_) => {}
                }
            }

            std::thread::sleep(if revealed { POLL_ACTIVE } else { POLL_IDLE });
        }
        crate::logln!("autohide sampler stopped");
    });

    running
}
