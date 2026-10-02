//! The notes run that starts on its own when a call ends (SPEC A11, TUR-10).
//!
//! After Stop, once `transcript.md` is final, [`finish_then_run`] starts the
//! user's agent CLI on the meeting, if `agent.auto_run` is on. The meeting
//! view shows *Writing notes…* with Cancel, then the notes, or why there are
//! none and Retry ([`start_notes_run`]). Each change goes out as
//! [`crate::events::AGENT_RUN_STATUS_EVENT`].
//!
//! - [`notes`]: one run, start to finish, with no Tauri in it.
//! - [`runs`]: one run per meeting at a time, each on its own thread.
//! - [`failure`]: every way a run can fail, in plain words.
//!
//! Quitting cancels every run ([`shutdown`]): the agent is stopped before it
//! answers, so the meeting is left as it was and Retry works on next launch.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};

use crate::error::UiError;
use crate::live_transcript::{self, Transcription};

mod failure;
mod notes;
mod runs;

pub use runs::AgentRuns;
use runs::{Sink, Work};

/// How long quitting waits for running agents to be stopped.
const QUIT_WAIT: Duration = Duration::from_secs(3);

/// Where one meeting's notes run stands, as the meeting view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The meeting folder name.
    pub meeting_id: String,
    pub state: State,
}

/// The run's state. `idle` means no run for this meeting since launch: the
/// view then goes by what is on disk (notes there or not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum State {
    Idle,
    /// *Writing notes…*, with Cancel.
    Running,
    /// The notes and tasks are on disk. `tasks` is how many tickets were
    /// written.
    Done {
        tasks: u32,
    },
    /// No notes were written, and why. Retry starts again.
    Failed {
        failure: Failure,
    },
}

/// Why a run wrote no notes, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailureKind,
    /// One or two sentences for the meeting view, next to Retry.
    pub message: String,
    /// A command to type in Terminal that fixes it, when there is one
    /// (signing in).
    pub command: Option<String>,
}

/// Every way a run can end without notes. One per thing the user can do
/// about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum FailureKind {
    /// No agent is set up (`agent.harness` is `none`); Copy prompt instead.
    NoAgent,
    /// The chosen agent CLI is not on this Mac, or meet-ai cannot find it.
    NotInstalled,
    /// The CLI is there but nobody is signed in to it.
    NotSignedIn,
    /// The run went past `agent.timeout_sec` and was stopped.
    TimedOut,
    /// The user pressed Cancel.
    Cancelled,
    /// The CLI exited with an error.
    CliFailed,
    /// The CLI's answer was not JSON, or not in the notes format.
    BadReply,
    /// The run could not be started (temp folder, launch, prompt template,
    /// config).
    CouldNotStart,
    /// The meeting has no transcript to write notes from.
    NoTranscript,
    /// The user switched notes off for this meeting (`agent_notes: off`).
    NotesOff,
    /// The answer was fine but writing `meeting.md` or the tickets failed.
    WriteFailed,
}

/// The agent-written half of `meeting.md`, for the meeting view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNotes {
    /// The meeting is marked `agent_notes: off`.
    pub notes_off: bool,
    /// `analyzed_by` (`claude-code`, `codex`, `clipboard`), `None` before any
    /// notes were written.
    pub analyzed_by: Option<String>,
    /// The four sections (Summary, Decisions, Action Items, Open Questions)
    /// that have text, in that order. Bodies are markdown as written.
    pub sections: Vec<NotesSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotesSection {
    pub heading: String,
    pub body: String,
}

/// Where this meeting's notes run stands.
#[tauri::command]
#[specta::specta]
pub async fn notes_run_status(app: AppHandle, meeting_id: String) -> Result<Status, UiError> {
    Ok(runs(&app)?.status(&meeting_id))
}

/// Start the notes run by hand: Retry, or the first run on a meeting that
/// has none. Ignored while one is running; the answer is then that run.
#[tauri::command]
#[specta::specta]
pub async fn start_notes_run(app: AppHandle, meeting_id: String) -> Result<Status, UiError> {
    blocking(move || {
        let root = crate::meetings::root()?;
        start(&app, root, &meeting_id)
    })
    .await
}

/// Cancel this meeting's running notes run. A no-op when none is running.
#[tauri::command]
#[specta::specta]
pub async fn cancel_notes_run(app: AppHandle, meeting_id: String) -> Result<Status, UiError> {
    Ok(runs(&app)?.cancel(&meeting_id))
}

/// The agent-written sections of this meeting's `meeting.md`.
#[tauri::command]
#[specta::specta]
pub async fn meeting_notes(meeting_id: String) -> Result<MeetingNotes, UiError> {
    blocking(move || read_meeting_notes(&crate::meetings::root()?, &meeting_id)).await
}

/// Switch "Make notes for this meeting" on or off (SPEC A11, "Skip one
/// meeting", TUR-12), and answer with the meeting's notes as they now stand.
///
/// Off writes `agent_notes: off` into `meeting.md` and cancels this
/// meeting's notes run if one is going, so nothing more is sent or written.
/// On removes the key; the view then offers *Make notes now*, which is
/// [`start_notes_run`].
#[tauri::command]
#[specta::specta]
pub async fn set_meeting_notes(
    app: AppHandle,
    meeting_id: String,
    on: bool,
) -> Result<MeetingNotes, UiError> {
    blocking(move || {
        let root = crate::meetings::root()?;
        let agent_runs = app.try_state::<AgentRuns>();
        switch_notes(
            &root,
            agent_runs.as_deref(),
            &meeting_id,
            on,
            &own_writes(&app),
        )
    })
    .await
}

/// Finish the meeting's transcription, then start its notes run if
/// `agent.auto_run` is on. The recorder's Stop calls this in place of
/// `Transcription::finish`; `transcript.md` is final once that returns.
///
/// The run has a thread of its own, so this returns at once after the
/// transcript is done and the next recording is never held up by it.
/// Starts nothing while the app quits.
pub fn finish_then_run(app: &AppHandle, transcription: Transcription) {
    let transcript = transcription.transcript().to_path_buf();
    transcription.finish(live_transcript::STOP_TIMEOUT);
    let Ok(agent_runs) = runs(app) else {
        return;
    };
    if agent_runs.is_closed() {
        return;
    }
    let Some((root, meeting_id)) = meeting_of(&transcript) else {
        tracing::warn!(path = %transcript.display(), "no meeting folder around the transcript; no notes run");
        return;
    };
    match crate::config::agent() {
        Ok(settings) if !settings.auto_run || settings.harness == crate::config::Harness::None => {}
        // A config that cannot be read still starts, so the meeting view
        // says why there are no notes.
        _ => {
            if let Err(error) = start(app, root, &meeting_id) {
                tracing::warn!(message = %error.message, "could not start the notes run");
            }
        }
    }
}

/// The app is quitting: start no more runs and stop the ones going.
pub fn shutdown(app: &AppHandle) {
    if let Ok(agent_runs) = runs(app) {
        agent_runs.shutdown(QUIT_WAIT);
    }
}

/// Start (or, while one is going, report) the notes run for `meeting_id`
/// under `root`.
fn start(app: &AppHandle, root: PathBuf, meeting_id: &str) -> Result<Status, UiError> {
    let self_writes = own_writes(app);
    let sink: Arc<dyn Sink> = Arc::new(AppSink {
        app: app.clone(),
        root: root.clone(),
    });
    let id = meeting_id.to_owned();
    let work: Work = Box::new(move |cancel| {
        notes::run_notes(&root, &id, notes::configured, cancel, &self_writes)
    });
    Ok(runs(app)?.start(meeting_id, work, sink))
}

/// The watcher's record of this process's own writes, so the files a run or
/// the switch writes do not come back as outside changes (SPEC §4).
fn own_writes(app: &AppHandle) -> store::watcher::SelfWrites {
    app.try_state::<crate::watch::MeetingsWatch>()
        .map(|watch| watch.own_writes().clone())
        .unwrap_or_default()
}

/// [`set_meeting_notes`] without Tauri: write the switch, cancel the running
/// notes run when it went off, and read the notes back.
///
/// The run is cancelled even when the switch could not be written: the user
/// asked for no notes. It is cancelled after the write, so the run sees the
/// switch and ends as notes-off rather than as cancelled.
fn switch_notes(
    root: &Path,
    agent_runs: Option<&AgentRuns>,
    meeting_id: &str,
    on: bool,
    self_writes: &store::watcher::SelfWrites,
) -> Result<MeetingNotes, UiError> {
    let switched = store::notes_switch::set(root, meeting_id, on, self_writes);
    if !on && let Some(agent_runs) = agent_runs {
        agent_runs.cancel(meeting_id);
    }
    switched?;
    read_meeting_notes(root, meeting_id)
}

fn runs(app: &AppHandle) -> Result<tauri::State<'_, AgentRuns>, UiError> {
    app.try_state::<AgentRuns>()
        .ok_or_else(|| UiError::app("no-agent-runs", "The notes runs are not set up."))
}

/// The meetings root and meeting id a `transcript.md` path belongs to.
fn meeting_of(transcript: &Path) -> Option<(PathBuf, String)> {
    let dir = transcript.parent()?;
    let id = dir.file_name()?.to_str()?.to_owned();
    Some((dir.parent()?.to_path_buf(), id))
}

/// `meeting.md`'s agent half, for [`meeting_notes`].
fn read_meeting_notes(root: &Path, meeting_id: &str) -> Result<MeetingNotes, UiError> {
    let dir = store::folder::meeting_dir(root, meeting_id)?;
    let Some(meeting) = store::meeting::Meeting::read(&dir.join(store::MEETING_FILE))? else {
        return Ok(MeetingNotes {
            notes_off: false,
            analyzed_by: None,
            sections: Vec::new(),
        });
    };
    let sections = store::meeting::SECTIONS
        .iter()
        .filter_map(|&heading| {
            let body = meeting.section(heading)?.trim_end();
            (!body.trim().is_empty()).then(|| NotesSection {
                heading: heading.to_owned(),
                body: body.to_owned(),
            })
        })
        .collect();
    Ok(MeetingNotes {
        notes_off: store::notes_switch::is_off(&meeting),
        analyzed_by: meeting.frontmatter.get_str("analyzed_by"),
        sections,
    })
}

/// Runs `work` on the blocking pool, off the thread the window waits on.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, UiError> + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))?
}

/// The window and the notification, as a [`Sink`].
struct AppSink {
    app: AppHandle,
    root: PathBuf,
}

impl Sink for AppSink {
    fn status(&self, status: &Status) {
        if let Err(error) = self.app.emit(crate::events::AGENT_RUN_STATUS_EVENT, status) {
            tracing::warn!(%error, "could not tell the window about the notes run");
        }
    }

    /// A notification, unless the window is in front and shows it anyway.
    fn notes_ready(&self, meeting_id: &str, tasks: u32) {
        let in_front = self
            .app
            .webview_windows()
            .values()
            .any(|window| window.is_focused().unwrap_or(false));
        if in_front {
            return;
        }
        let title = store::folder::meeting_dir(&self.root, meeting_id)
            .ok()
            .and_then(|dir| store::meeting::Meeting::read(&dir.join(store::MEETING_FILE)).ok())
            .flatten()
            .and_then(|meeting| meeting.title())
            .unwrap_or_else(|| meeting_id.to_owned());
        crate::notify::notes_ready(&self.app, &title, tasks);
    }
}

#[cfg(test)]
mod tests;
