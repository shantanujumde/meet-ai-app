//! The recording lifecycle: start, stop, and what ⌘⇧R does.
//!
//! The recorder behind this is real (TUR-94's `audio::session::RecordingSession`):
//! start opens the microphone and, where available, the system-audio tap and
//! writes into the SPEC §3.1 meeting folder this module creates; stop closes
//! both cleanly and leaves `mic.wav`, `system.wav` and `segments.json` complete,
//! with no later repair step needed.
//!
//! Starting and stopping both take real wall-clock time — SPEC §8.1's
//! positive-control permission measurement, then Core Audio warming up each
//! channel — so every entry point here is meant to be called off a thread that
//! must stay responsive. `commands::toggle_recording`/`stop_recording` run this
//! on a blocking thread the same way `commands::permission_status` does; the
//! ⌘⇧R handler in `lib.rs` gives it a worker thread of its own so the shortcut
//! callback never blocks. Nothing here needs to know which caller it is: the
//! phase check and transition happen inside one `Mutex`, so whichever caller's
//! lock lands first wins a race and the other sees `Starting`/`Stopping` and
//! no-ops, rather than racing a second tap open.

use std::sync::Mutex;

use audio::AudioSource;
use audio::session::RecordingSession;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _};

use crate::error::UiError;
use crate::permission;

/// The Tauri event the frontend subscribes to. Every transition emits one, so
/// the UI never has to poll and the menu bar, the titlebar and the sidebar all
/// see the same change at the same time.
pub const STATE_EVENT: &str = "recording://state";

/// Where the recorder is right now.
///
/// `Starting` and `Stopping` are not decoration: opening the tap (and, on
/// start, the SPEC §8.1 permission measurement ahead of it) and flushing the
/// last WAV header both take long enough to see, and a shortcut pressed twice
/// in that window must be ignored rather than queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Idle,
    Starting,
    Recording,
    Stopping,
}

/// The recorder's state as the webview sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    /// The folder name of the meeting being recorded, `None` when idle.
    pub meeting_id: Option<String>,
    /// Unix epoch milliseconds the recording started, so the UI can run its own
    /// timer instead of being fed a tick per second over IPC.
    pub started_at_ms: Option<i64>,
}

impl Status {
    fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            meeting_id: None,
            started_at_ms: None,
        }
    }
}

/// The recorder's status plus the live capture session behind it, one Mutex
/// so a phase transition and the session it owns can never observe each other
/// half-updated.
struct Inner {
    status: Status,
    session: Option<RecordingSession>,
}

impl Inner {
    fn idle() -> Self {
        Self {
            status: Status::idle(),
            session: None,
        }
    }
}

/// Managed Tauri state. One recorder per app, because two would fight over the
/// system audio tap — the same reason the single-instance plugin is wired up.
pub struct Recorder {
    inner: Mutex<Inner>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::idle()),
        }
    }
}

impl Recorder {
    pub fn status(&self) -> Status {
        self.lock().status.clone()
    }

    /// A poisoned lock here means a previous call panicked while holding it.
    /// Recovering is strictly better than taking the whole app down; the worst
    /// case is a stuck phase, not a half-broken invariant.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Mutate the state and tell the whole app about it in one step, so a
    /// transition can never land without an event.
    fn transition(&self, app: &AppHandle, change: impl FnOnce(&mut Inner)) -> Status {
        let next = {
            let mut inner = self.lock();
            change(&mut inner);
            inner.status.clone()
        };
        // A webview that has gone away is not an error worth propagating up
        // into a recording control.
        if let Err(error) = app.emit(STATE_EVENT, &next) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        next
    }

    /// Start if idle, stop if recording, and do nothing mid-transition.
    ///
    /// One entry point for the button, the menu item and the global shortcut,
    /// so all three cannot disagree about what "toggle" means.
    pub fn toggle(&self, app: &AppHandle) -> Result<Status, UiError> {
        match self.status().phase {
            Phase::Idle => self.start(app),
            Phase::Recording => self.stop(app),
            // Mid-transition. Returning the current state rather than an error
            // is correct: the user pressed a toggle and the answer is "already
            // doing that", which is not a failure worth interrupting them for.
            Phase::Starting | Phase::Stopping => Ok(self.status()),
        }
    }

    /// Claim `Idle -> Starting`, or report the phase that beat us to it.
    fn claim_starting(&self, app: &AppHandle) -> Result<(), Status> {
        let mut inner = self.lock();
        if inner.status.phase != Phase::Idle {
            return Err(inner.status.clone());
        }
        inner.status.phase = Phase::Starting;
        let status = inner.status.clone();
        drop(inner);
        if let Err(error) = app.emit(STATE_EVENT, &status) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        Ok(())
    }

    pub fn start(&self, app: &AppHandle) -> Result<Status, UiError> {
        if let Err(status) = self.claim_starting(app) {
            return Ok(status);
        }

        // SPEC §8.1: a denied tap returns `noErr` and delivers bit-exact
        // zeros, so a return code cannot tell us anything — only the positive
        // control (a tone played and listened for) can. The UI already keeps
        // the controls disabled while permission is absent; this measurement
        // is the backstop for the one path that has no button to disable, the
        // global shortcut firing with the window unfocused or hidden.
        if permission::measure().state == permission::State::Denied {
            return Err(self.fail_start(
                app,
                None,
                UiError::app(
                    "permission-denied",
                    "meet-ai is not allowed to record this Mac's audio, so starting a recording \
                     would capture nothing but silence.",
                ),
            ));
        }

        let started = chrono::Local::now();
        let id = meeting_id(started);
        if let Err(error) = create_meeting_folder(&id) {
            return Err(self.fail_start(app, Some(&id), error));
        }

        let audio_dir = match crate::meetings::root() {
            Ok(root) => root.join(&id).join("audio"),
            Err(error) => return Err(self.fail_start(app, Some(&id), error)),
        };

        let mic: Box<dyn AudioSource> = Box::new(audio::mic::MicSource::new());
        let sys = audio::session::default_system_source();

        match RecordingSession::start(audio_dir, mic, sys) {
            Ok(session) => Ok(self.transition(app, |inner| {
                inner.status.phase = Phase::Recording;
                inner.status.meeting_id = Some(id.clone());
                inner.status.started_at_ms = Some(started.timestamp_millis());
                inner.session = Some(session);
            })),
            Err(message) => {
                Err(self.fail_start(app, Some(&id), UiError::app("recorder-failed", message)))
            }
        }
    }

    /// Back to idle, never stuck in `Starting`, and never leaving a meeting
    /// folder with no audio in it behind — sitting in `Recording` with nothing
    /// written is the exact shape of the TUR-90 report; a folder from a start
    /// that never got that far is the same failure one step earlier.
    fn fail_start(&self, app: &AppHandle, id: Option<&str>, error: UiError) -> UiError {
        self.transition(app, |inner| *inner = Inner::idle());
        if let Some(id) = id {
            match crate::meetings::root() {
                Ok(root) => {
                    if let Err(remove_error) = std::fs::remove_dir_all(root.join(id)) {
                        tracing::warn!(
                            %remove_error,
                            meeting_id = id,
                            "could not remove the meeting folder left by a failed recording start"
                        );
                    }
                }
                Err(root_error) => tracing::warn!(
                    message = %root_error.message,
                    "could not resolve the meetings root to clean up a failed recording start"
                ),
            }
        }
        error
    }

    pub fn stop(&self, app: &AppHandle) -> Result<Status, UiError> {
        let session = match self.claim_stopping(app) {
            Err(status) => return Ok(status),
            Ok(session) => session,
        };

        let Some(session) = session else {
            tracing::error!("phase was Recording with no session attached; recovering to idle");
            return Ok(self.transition(app, |inner| *inner = Inner::idle()));
        };

        match session.stop() {
            Ok(_report) => Ok(self.transition(app, |inner| *inner = Inner::idle())),
            Err(message) => {
                tracing::warn!(message = %message, "recording did not stop cleanly");
                self.transition(app, |inner| *inner = Inner::idle());
                Err(UiError::app("recorder-failed", message))
            }
        }
    }

    /// Claim `Recording -> Stopping` and take the session with it, or report
    /// the phase that beat us to it.
    fn claim_stopping(&self, app: &AppHandle) -> Result<Option<RecordingSession>, Status> {
        let mut inner = self.lock();
        if inner.status.phase != Phase::Recording {
            return Err(inner.status.clone());
        }
        inner.status.phase = Phase::Stopping;
        let session = inner.session.take();
        let status = inner.status.clone();
        drop(inner);
        if let Err(error) = app.emit(STATE_EVENT, &status) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        Ok(session)
    }
}

/// Build a SPEC §3.1 meeting id: `YYYY-MM-DD-HHMM-slug`.
///
/// The slug is `meeting` until Phase 5a can name it from the calendar event.
/// A fixed slug is better than a guessed one — the list falls back to showing
/// the date and time, which is true, instead of a title nobody chose.
fn meeting_id(at: chrono::DateTime<chrono::Local>) -> String {
    format!("{}-meeting", at.format("%Y-%m-%d-%H%M"))
}

/// Create the meeting folder and the files SPEC §3.1 says live in it.
///
/// `transcript.md` is created empty and never written to here: §3.4 makes it
/// append-only and `crates/stt`'s `TranscriptSink` is the only thing allowed to
/// append. Creating it up front means the review view can open a meeting that
/// is still recording without a missing-file branch. `audio/` is also created
/// here, ahead of `RecordingSession::start`'s own (idempotent)
/// `create_dir_all`, so folder creation stays one step even though the audio
/// inside it is now the session's to write.
fn create_meeting_folder(id: &str) -> Result<(), UiError> {
    let dir = crate::meetings::root()?.join(id);
    std::fs::create_dir_all(dir.join("audio"))?;
    for file in ["transcript.md", "notes.md"] {
        let path = dir.join(file);
        if !path.exists() {
            std::fs::write(&path, "")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    #[test]
    fn meeting_ids_match_the_spec_3_1_folder_name() {
        let at = chrono::Local
            .with_ymd_and_hms(2026, 9, 1, 14, 30, 0)
            .single()
            .expect("a real local time");
        let id = meeting_id(at);
        assert_eq!(id, "2026-09-01-1430-meeting");

        // The list parser has to be able to read back what the recorder writes.
        // If these two ever disagree, every new recording shows up undated at
        // the bottom of the list and nobody notices until there are several.
        let (date, time, slug) = crate::meetings::split_folder_name(&id);
        assert_eq!(date.as_deref(), Some("2026-09-01"));
        assert_eq!(time.as_deref(), Some("14:30"));
        assert_eq!(slug.as_deref(), Some("meeting"));
    }

    #[test]
    fn a_fresh_recorder_is_idle() {
        let recorder = Recorder::default();
        let status = recorder.status();
        assert_eq!(status.phase, Phase::Idle);
        assert!(status.meeting_id.is_none());
        assert!(status.started_at_ms.is_none());
    }
}
