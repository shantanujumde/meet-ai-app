//! The notes run that starts on its own when a call ends (SPEC A11, TUR-10).
//!
//! SKELETON: the public types and command signatures below are the contract
//! the window is built against. The bodies are filled in by the TUR-10 Rust
//! work; keep the names and shapes.

use serde::Serialize;

use crate::error::UiError;

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
    Done { tasks: u32 },
    /// No notes were written, and why. Retry starts again.
    Failed { failure: Failure },
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
pub async fn notes_run_status(meeting_id: String) -> Result<Status, UiError> {
    Ok(Status {
        meeting_id,
        state: State::Idle,
    })
}

/// Start the notes run by hand: Retry, or the first run on a meeting that
/// has none. Ignored while one is running; the answer is then that run.
#[tauri::command]
#[specta::specta]
pub async fn start_notes_run(meeting_id: String) -> Result<Status, UiError> {
    Ok(Status {
        meeting_id,
        state: State::Idle,
    })
}

/// Cancel this meeting's running notes run. A no-op when none is running.
#[tauri::command]
#[specta::specta]
pub async fn cancel_notes_run(meeting_id: String) -> Result<Status, UiError> {
    Ok(Status {
        meeting_id,
        state: State::Idle,
    })
}

/// The agent-written sections of this meeting's `meeting.md`.
#[tauri::command]
#[specta::specta]
pub async fn meeting_notes(meeting_id: String) -> Result<MeetingNotes, UiError> {
    let _ = meeting_id;
    Ok(MeetingNotes {
        notes_off: false,
        analyzed_by: None,
        sections: Vec::new(),
    })
}
