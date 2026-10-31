//! Whatever is playing, through Windows' own media session.
//!
//! `GlobalSystemMediaTransportControlsSessionManager` is the same source the Windows
//! volume flyout and the lock screen read, so the widget shows the same track and the
//! same buttons do the same thing - no per-app integration, and nothing is intercepted.
//!
//! Everything here runs on one background thread initialised **multi-threaded**:
//! these are WinRT async calls, and blocking on `.get()` from the app's
//! apartment-threaded main thread would deadlock waiting for a completion that needs
//! that same thread to pump messages. The thread keeps a snapshot the UI reads as
//! plain data - the same rule the auto-hide sampler follows for the same reason.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    /// False when nothing at all is playing; the widget then shows a resting state
    /// rather than empty controls.
    pub active: bool,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// App that owns the session, e.g. "Spotify" - shown small, under the title.
    pub source: String,
}

static SNAPSHOT: Mutex<Option<NowPlaying>> = Mutex::new(None);

/// Pending transport command: 0 none, 1 play/pause, 2 next, 3 previous.
static PENDING: AtomicU8 = AtomicU8::new(0);

pub fn now_playing() -> NowPlaying {
    SNAPSHOT.lock().unwrap().clone().unwrap_or_default()
}

/// Queue a transport command for the media thread. `what` is "toggle", "next" or
/// "previous"; anything else is ignored.
pub fn control(what: &str) {
    let code = match what {
        "toggle" => 1,
        "next" => 2,
        "previous" => 3,
        _ => return,
    };
    PENDING.store(code, Ordering::SeqCst);
}

/// Start the poller. Safe to call once at startup; it stops when the process exits.
pub fn start() {
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        unsafe {
            // MTA: see the module comment. S_FALSE (already initialised) is fine.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        crate::logln!("media session poller started");

        loop {
            let pending = PENDING.swap(0, Ordering::SeqCst);
            if pending != 0 {
                if let Err(e) = run_control(pending) {
                    crate::logln!("media control {pending} failed: {e}");
                }
            }

            let next = read().unwrap_or_default();
            let mut slot = SNAPSHOT.lock().unwrap();
            if slot.as_ref() != Some(&next) {
                *slot = Some(next);
            }
            drop(slot);

            // A transport command should feel immediate, so the loop ticks four times
            // a second but only touches WinRT when something changed or a command is
            // waiting. Measured idle cost stays inside the noise.
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

fn current_session(
) -> windows::core::Result<windows::Media::Control::GlobalSystemMediaTransportControlsSession> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
    Manager::RequestAsync()?.get()?.GetCurrentSession()
}

fn read() -> windows::core::Result<NowPlaying> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;

    let session = match current_session() {
        Ok(s) => s,
        // No session at all is the normal "nothing is playing" case, not an error.
        Err(_) => return Ok(NowPlaying::default()),
    };

    let props = session.TryGetMediaPropertiesAsync()?.get()?;
    let title = props.Title().map(|s| s.to_string()).unwrap_or_default();
    let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();

    let playing = session
        .GetPlaybackInfo()
        .and_then(|i| i.PlaybackStatus())
        .map(|s| s == Status::Playing)
        .unwrap_or(false);

    let source = session
        .SourceAppUserModelId()
        .map(|s| pretty_source(&s.to_string()))
        .unwrap_or_default();

    Ok(NowPlaying {
        active: !title.is_empty() || playing,
        title,
        artist,
        playing,
        source,
    })
}

/// Turn an AUMID into something a person would recognise: `Spotify.exe` ->
/// `Spotify`, `Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic` -> `ZuneMusic`.
fn pretty_source(aumid: &str) -> String {
    // "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" -> the part after '!'
    let tail = aumid.rsplit('!').next().unwrap_or(aumid);
    // drop a package hash, if there is one
    let tail = tail.split('_').next().unwrap_or(tail);
    // "Spotify.exe" -> "Spotify". This has to happen BEFORE splitting on '.', or
    // the last segment is "exe" and the name is thrown away - which is exactly what
    // the unit test caught.
    let tail = if tail.to_ascii_lowercase().ends_with(".exe") {
        &tail[..tail.len() - 4]
    } else {
        tail
    };
    // "Microsoft.ZuneMusic" -> "ZuneMusic"
    tail.rsplit('.').next().unwrap_or(tail).to_string()
}

fn run_control(code: u8) -> windows::core::Result<()> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;

    let session = current_session()?;
    let ok = match code {
        1 => {
            // Not `TryTogglePlayPauseAsync`. Plenty of sessions expose Play and Pause
            // without enabling the toggle, and the call then returns false and does
            // nothing at all - measured against Chromium, where next-track worked and
            // the toggle silently did not. Ask for the specific transition instead,
            // and keep the toggle only as a fallback.
            let playing = session
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus())
                .map(|s| s == Status::Playing)
                .unwrap_or(false);
            let done = if playing {
                session.TryPauseAsync()?.get()?
            } else {
                session.TryPlayAsync()?.get()?
            };
            if done {
                true
            } else {
                session.TryTogglePlayPauseAsync()?.get()?
            }
        }
        2 => session.TrySkipNextAsync()?.get()?,
        3 => session.TrySkipPreviousAsync()?.get()?,
        _ => true,
    };
    if !ok {
        crate::logln!("media: the session refused control {code}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_names_are_readable() {
        assert_eq!(super::pretty_source("Spotify.exe"), "Spotify");
        assert_eq!(
            super::pretty_source("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
        assert_eq!(super::pretty_source("chrome.exe"), "chrome");
        // what Edge actually reports, seen during PHASE 5 testing
        assert_eq!(super::pretty_source("MSEdge"), "MSEdge");
        assert_eq!(super::pretty_source("Spotify.EXE"), "Spotify");
    }
}
