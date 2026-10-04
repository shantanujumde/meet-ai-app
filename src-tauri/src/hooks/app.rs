//! When each user hook runs (TUR-63), and the "hook failed" note.
//!
//! - `on_meeting_end`: the recorder goes back to idle after a meeting.
//! - `on_transcript_ready`: `transcript.md` is final
//!   ([`crate::retention::Transcribing::settled`] calls [`fire`]).
//! - `on_analysis_complete`: the notes run wrote its notes.
//!
//! The first and last follow the events the window already gets, so the
//! recorder and the notes run need no change. Every hook runs on a thread of
//! its own and only ever logs and emits [`crate::events::HOOK_FAILED_EVENT`].

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Listener as _};

use super::Moment;
use crate::events::{AGENT_RUN_STATUS_EVENT, HOOK_FAILED_EVENT, RECORDING_STATE_EVENT};
use crate::lock::lock_or_recover;

/// What [`HOOK_FAILED_EVENT`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HookFailed {
    /// The meeting folder name.
    pub meeting_id: String,
    /// The config key: `on_transcript_ready`, `on_analysis_complete` or
    /// `on_meeting_end`.
    pub hook: String,
    /// What went wrong, in a few words.
    pub message: String,
}

/// Start following the recorder and the notes runs.
pub fn init(app: &AppHandle) {
    let recording = Arc::new(Mutex::new(None::<String>));
    let handle = app.clone();
    app.listen_any(RECORDING_STATE_EVENT, move |event| {
        let now = meeting_id_of(event.payload());
        let ended = {
            let mut last = lock_or_recover(&recording);
            meeting_ended(&mut last, now)
        };
        if let Some(id) = ended {
            fire(&handle, Moment::MeetingEnd, &id);
        }
    });
    let handle = app.clone();
    app.listen_any(AGENT_RUN_STATUS_EVENT, move |event| {
        if let Some(id) = notes_done(event.payload()) {
            fire(&handle, Moment::AnalysisComplete, &id);
        }
    });
}

/// Run `moment`'s hook for `meeting_id` on a thread of its own.
pub fn fire(app: &AppHandle, moment: Moment, meeting_id: &str) {
    let config = crate::config::hooks();
    if moment.command(&config).is_none() {
        return;
    }
    let app = app.clone();
    let meeting_id = meeting_id.to_owned();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-hook".to_owned())
        .spawn(move || {
            let Some(dir) = crate::meetings::root()
                .ok()
                .and_then(|root| super::meeting_dir(&root, &meeting_id))
            else {
                tracing::warn!(hook = moment.name(), "no meeting folder for the hook");
                return;
            };
            if let Some(report) = super::run_moment(&config, moment, &dir) {
                let failed = HookFailed {
                    meeting_id,
                    hook: moment.name().to_owned(),
                    message: report.outcome.describe(),
                };
                if let Err(error) = app.emit(HOOK_FAILED_EVENT, &failed) {
                    tracing::warn!(%error, "could not tell the window a hook failed");
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, hook = moment.name(), "could not start the hook");
    }
}

/// `meetingId` in a recorder status payload.
fn meeting_id_of(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    value.get("meetingId")?.as_str().map(str::to_owned)
}

/// The meeting that just ended, if the recorder went from one to none.
pub(super) fn meeting_ended(last: &mut Option<String>, now: Option<String>) -> Option<String> {
    match now {
        Some(id) => {
            *last = Some(id);
            None
        }
        None => last.take(),
    }
}

/// The meeting whose notes run just wrote its notes, from its status payload.
pub(super) fn notes_done(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let state = value.get("state")?.get("state")?.as_str()?;
    (state == "done")
        .then(|| value.get("meetingId")?.as_str().map(str::to_owned))
        .flatten()
}
