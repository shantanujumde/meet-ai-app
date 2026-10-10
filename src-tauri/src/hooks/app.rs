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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Listener as _, Manager as _};

use super::Moment;
use crate::error::UiError;
use crate::events::{AGENT_RUN_STATUS_EVENT, HOOK_FAILED_EVENT, RECORDING_STATE_EVENT};
use crate::folder_move::FolderGate;
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

/// The gate's refusal while the meetings folder moves.
const FOLDER_MOVING: &str = "folder-move-in-progress";

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
            let gate = app.try_state::<FolderGate>();
            let ran = run_gated(gate.as_deref(), crate::meetings::root, &config, moment, &meeting_id);
            let message = match ran {
                Ok(None) => return,
                Ok(Some(report)) => report.outcome.describe(),
                Err(refused) if refused.kind == FOLDER_MOVING => {
                    tracing::warn!(hook = moment.name(), "hook not run: the folder is moving");
                    "did not run: the meetings folder was moving".to_owned()
                }
                Err(error) => {
                    tracing::warn!(hook = moment.name(), error = %error.message, "no meetings folder for the hook");
                    return;
                }
            };
            let failed = HookFailed {
                meeting_id,
                hook: moment.name().to_owned(),
                message,
            };
            if let Err(error) = app.emit(HOOK_FAILED_EVENT, &failed) {
                tracing::warn!(%error, "could not tell the window a hook failed");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, hook = moment.name(), "could not start the hook");
    }
}

/// Run the hook holding the folder gate for its whole run (TUR-167): it works
/// in the meeting folder, often writes there, and a folder move meanwhile
/// would copy past it and delete what it wrote. The gate only counts writers,
/// so holding it never blocks the window; a move started meanwhile is refused
/// with `folder-busy`, and a hook due during a move is refused, not run.
/// `Ok(Some)` is a hook that did not succeed.
pub(super) fn run_gated(
    gate: Option<&FolderGate>,
    root: impl FnOnce() -> Result<PathBuf, UiError>,
    config: &crate::config::HooksConfig,
    moment: Moment,
    meeting_id: &str,
) -> Result<Option<super::Report>, UiError> {
    crate::folder_move::writing_in(gate, root, |root| {
        let Some(dir) = super::meeting_dir(root, meeting_id) else {
            tracing::warn!(hook = moment.name(), "no meeting folder for the hook");
            return Ok(None);
        };
        Ok(super::run_moment(config, moment, &dir))
    })
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
