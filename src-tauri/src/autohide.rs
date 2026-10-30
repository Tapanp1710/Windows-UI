//! Reveal/conceal logic for the dock.
//!
//! Approach: a background thread samples the cursor position and decides whether the
//! dock should be showing. It emits `dock:reveal` / `dock:conceal` to the frontend,
//! which runs the actual slide as a CSS transform.
//!
//! Why polling rather than a mouse hook: a `WH_MOUSE_LL` hook sits in the input path
//! of every mouse message on the system, and a slow or wedged hook degrades input for
//! the whole machine. The core rule for this project is that Windows must keep
//! behaving exactly as it does normally, so the cheap, inert option wins. Sampling
//! `GetCursorPos` 20 times a second costs a couple of microseconds per sample and
//! cannot affect anyone else's input.
//!
//! The thread also parks itself (longer sleep) while concealed and idle, so the idle
//! CPU figure stays effectively zero.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

/// How close to the bottom edge the pointer must get to summon the dock.
/// Windows uses a 1px trigger for an auto-hidden taskbar; 2px is the same gesture
/// with a little tolerance for high-DPI rounding.
const TRIGGER_PX: i32 = 2;

/// Delay before hiding once the pointer leaves, per the brief.
const CONCEAL_DELAY: Duration = Duration::from_millis(400);

/// Sampling intervals. Slower while hidden, since nothing is on screen to feel laggy.
const POLL_IDLE: Duration = Duration::from_millis(60);
const POLL_ACTIVE: Duration = Duration::from_millis(40);

/// Set once the user turns on "always visible" in settings.
pub static ALWAYS_VISIBLE: AtomicBool = AtomicBool::new(false);

pub fn start(app: AppHandle) {
    let running = Arc::new(AtomicBool::new(true));
    let stop = running.clone();

    // Stop the thread when the app exits so a lingering thread cannot keep the
    // process alive after the window is gone.
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut leave_at: Option<Instant> = None;

        // Start concealed: the dock should be out of the way until summoned.
        let _ = handle.emit("dock:conceal", ());
        let mut revealed = false;

        while stop.load(Ordering::Relaxed) {
            let always = ALWAYS_VISIBLE.load(Ordering::Relaxed);
            let (_, cy) = crate::win::cursor_pos();
            let (_, screen_h) = crate::win::taskbar::screen_size();

            // "Pointer is over the dock" is answered by the dock window's own rect so
            // the dock does not vanish while the pointer is still on it.
            let over_dock = handle
                .get_webview_window("dock")
                .and_then(|w| {
                    let pos = w.outer_position().ok()?;
                    let size = w.outer_size().ok()?;
                    let (cx, cy) = crate::win::cursor_pos();
                    Some(
                        cx >= pos.x
                            && cx < pos.x + size.width as i32
                            && cy >= pos.y
                            && cy < pos.y + size.height as i32,
                    )
                })
                .unwrap_or(false);

            let at_edge = cy >= screen_h - TRIGGER_PX;
            let want = always || at_edge || (revealed && over_dock);

            if want {
                leave_at = None;
                if !revealed {
                    revealed = true;
                    let _ = handle.emit("dock:reveal", ());
                }
            } else if revealed {
                match leave_at {
                    None => leave_at = Some(Instant::now() + CONCEAL_DELAY),
                    Some(t) if Instant::now() >= t => {
                        revealed = false;
                        leave_at = None;
                        let _ = handle.emit("dock:conceal", ());
                    }
                    Some(_) => {}
                }
            }

            std::thread::sleep(if revealed { POLL_ACTIVE } else { POLL_IDLE });
        }
    });

    // Flip the flag on exit.
    app.manage(AutohideHandle(running));
}

/// Dropped when Tauri tears down managed state, which stops the thread.
pub struct AutohideHandle(Arc<AtomicBool>);

impl Drop for AutohideHandle {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}
