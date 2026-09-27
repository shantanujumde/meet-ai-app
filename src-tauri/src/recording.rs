//! The recording lifecycle: start, stop, and what ⌘⇧R does.
//!
//! The **state machine is real and lands now**; the recorder behind it is a
//! stub until TUR-4 wires `crates/audio`'s `meet-rec` in. That split is
//! deliberate — the interesting bugs here are the transitions (a double-fire
//! from the global shortcut, a stop that races a start, a start attempted
//! without permission), and they are cheaper to get right against a stub than
//! against a live Core Audio tap.
//!
//! What the stub does do is create the SPEC §3.1 meeting folder, so the rest of
//! the shell has something real to list, open and take notes against. It writes
//! **no audio and no transcript lines** — inventing either would be worse than
//! useless. When TUR-4 lands, folder creation moves into the recorder and this
//! file keeps only the transitions.

use std::sync::Mutex;

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
/// `Starting` and `Stopping` are not decoration: once a real recorder is behind
/// this, opening the tap and flushing the last WAV header both take long enough
/// to see, and a shortcut pressed twice in that window must be ignored rather
/// than queued.
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
    /// No real capture is running behind this state. The UI says so plainly
    /// rather than letting someone believe a meeting is being recorded.
    pub stub: bool,
}

impl Status {
    fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            meeting_id: None,
            started_at_ms: None,
            stub: true,
        }
    }
}

/// Managed Tauri state. One recorder per app, because two would fight over the
/// system audio tap — the same reason the single-instance plugin is wired up.
pub struct Recorder {
    status: Mutex<Status>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            status: Mutex::new(Status::idle()),
        }
    }
}

impl Recorder {
    pub fn status(&self) -> Status {
        self.lock().clone()
    }

    /// A poisoned lock here means a previous call panicked while holding it.
    /// The state is a plain struct with no invariant a panic could half-break,
    /// so recovering is strictly better than taking the whole app down.
    fn lock(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Start if idle, stop if recording, and do nothing mid-transition.
    ///
    /// One entry point for the button, the menu item and the global shortcut,
    /// so all three cannot disagree about what "toggle" means.
    pub fn toggle(&self, app: &AppHandle) -> Result<Status, UiError> {
        let phase = self.lock().phase;
        match phase {
            Phase::Idle => self.start(app),
            Phase::Recording => self.stop(app),
            // Mid-transition. Returning the current state rather than an error
            // is correct: the user pressed a toggle and the answer is "already
            // doing that", which is not a failure worth interrupting them for.
            Phase::Starting | Phase::Stopping => Ok(self.status()),
        }
    }

    pub fn start(&self, app: &AppHandle) -> Result<Status, UiError> {
        if self.lock().phase != Phase::Idle {
            return Ok(self.status());
        }

        // Refuse rather than record silence. SPEC §8.1 says the controls stay
        // visibly disabled while permission is absent instead of failing at
        // click time, so the UI should never let this fire — but the global
        // shortcut works with the window unfocused and hidden, where there is
        // no disabled button to stop anyone.
        if permission::status().state == permission::State::Denied {
            return Err(UiError::app(
                "permission-denied",
                "meet-ai is not allowed to record this Mac's audio, so starting a recording would \
                 capture nothing but silence.",
            ));
        }

        self.set(app, |status| {
            status.phase = Phase::Starting;
        });

        let started = chrono::Local::now();
        let id = meeting_id(started);
        match create_meeting_folder(&id) {
            Ok(()) => {}
            Err(error) => {
                // Back to idle, not stuck in Starting. A failed start that
                // leaves the UI showing a spinner forever is the worst outcome.
                self.set(app, |status| *status = Status::idle());
                return Err(error);
            }
        }

        Ok(self.set(app, |status| {
            status.phase = Phase::Recording;
            status.meeting_id = Some(id);
            status.started_at_ms = Some(started.timestamp_millis());
        }))
    }

    pub fn stop(&self, app: &AppHandle) -> Result<Status, UiError> {
        if self.lock().phase != Phase::Recording {
            return Ok(self.status());
        }
        self.set(app, |status| {
            status.phase = Phase::Stopping;
        });
        // A real stop flushes the last WAV header and closes the tap here.
        Ok(self.set(app, |status| *status = Status::idle()))
    }

    /// Mutate the state and tell the whole app about it in one step, so a
    /// transition can never land without an event.
    fn set(&self, app: &AppHandle, change: impl FnOnce(&mut Status)) -> Status {
        let next = {
            let mut status = self.lock();
            change(&mut status);
            status.clone()
        };
        // A webview that has gone away is not an error worth propagating up
        // into a recording control.
        if let Err(error) = app.emit(STATE_EVENT, &next) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        next
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
/// is still recording without a missing-file branch.
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
    fn a_fresh_recorder_is_idle_and_honest_about_being_a_stub() {
        let recorder = Recorder::default();
        let status = recorder.status();
        assert_eq!(status.phase, Phase::Idle);
        assert!(status.meeting_id.is_none());
        assert!(status.started_at_ms.is_none());
        assert!(
            status.stub,
            "until TUR-4 lands, the UI must be able to say no real capture is running"
        );
    }
}
