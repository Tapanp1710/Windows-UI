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
/// Album art for the current track, as a data URL, with the title it belongs to.
///
/// Kept apart from the snapshot because it is tens of kilobytes: the island polls
/// the snapshot every couple of seconds and asks for the art only when the title it
/// is showing has changed.
static ART: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Pending transport command: 0 none, 1 play/pause, 2 next, 3 previous.
static PENDING: AtomicU8 = AtomicU8::new(0);

pub fn now_playing() -> NowPlaying {
    SNAPSHOT.lock().unwrap().clone().unwrap_or_default()
}

/// Album art for the track currently in the snapshot, if we have it yet.
pub fn art() -> Option<String> {
    ART.lock().unwrap().as_ref().map(|(_, url)| url.clone())
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
            let title_changed = SNAPSHOT
                .lock()
                .unwrap()
                .as_ref()
                .map(|p| p.title != next.title)
                .unwrap_or(true);

            let mut slot = SNAPSHOT.lock().unwrap();
            if slot.as_ref() != Some(&next) {
                *slot = Some(next.clone());
            }
            drop(slot);

            // Artwork is the one expensive read here - a stream copy of a bitmap -
            // so it only happens when the track actually changed.
            if title_changed {
                let url = if next.active {
                    read_art().ok().flatten()
                } else {
                    None
                };
                *ART.lock().unwrap() = url.map(|u| (next.title.clone(), u));
            }

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
/// Pull the session's thumbnail out as a data URL.
///
/// The media session hands back a stream reference rather than bytes, so this opens
/// it, reads it whole (artwork is a few tens of kilobytes) and base64s it. Returning
/// a data URL means the page needs no file access and nothing is written to disk.
fn read_art() -> windows::core::Result<Option<String>> {
    use base64::Engine;
    use windows::Storage::Streams::DataReader;

    let session = match current_session() {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let props = session.TryGetMediaPropertiesAsync()?.get()?;
    let Ok(thumb) = props.Thumbnail() else {
        return Ok(None);
    };

    let stream = thumb.OpenReadAsync()?.get()?;
    let size = stream.Size()? as u32;
    if size == 0 || size > 4 * 1024 * 1024 {
        return Ok(None);
    }
    let mime = stream.ContentType().map(|c| c.to_string()).unwrap_or_default();
    let mime = if mime.is_empty() { "image/png".to_string() } else { mime };

    let reader = DataReader::CreateDataReader(&stream)?;
    reader.LoadAsync(size)?.get()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(Some(format!("data:{mime};base64,{b64}")))
}

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
