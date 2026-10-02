//! Every way a notes run can end without notes, in the plain words the
//! meeting view shows next to Retry.

use agent::AgentError;

use super::{Failure, FailureKind};
use crate::error::UiError;

/// The most of the CLI's error text shown in the meeting view.
const STDERR_LIMIT: usize = 400;

/// The name the user knows a harness by.
pub fn display_name(harness_id: &str) -> &str {
    match harness_id {
        agent::claude::ID => agent::claude::DISPLAY_NAME,
        agent::codex::ID => "Codex",
        other => other,
    }
}

/// What to type in Terminal to sign in to this harness.
fn sign_in_command(harness_id: &str) -> &'static str {
    if harness_id == agent::codex::ID {
        "codex login"
    } else {
        "claude"
    }
}

fn failure(kind: FailureKind, message: impl Into<String>) -> Failure {
    Failure {
        kind,
        message: message.into(),
        command: None,
    }
}

/// An error from the agent crate, for the harness with id `harness_id`.
pub fn from_agent(error: &AgentError, harness_id: &str) -> Failure {
    let name = display_name(harness_id);
    match error {
        AgentError::NotInstalled { .. } => failure(
            FailureKind::NotInstalled,
            format!(
                "{name} is not installed, or meet-ai cannot find it. Install it, or set where \
                 it is in Settings, then press Retry."
            ),
        ),
        AgentError::NotSignedIn { .. } => {
            let command = sign_in_command(harness_id);
            let how = if command == "claude" {
                "Open Terminal, run claude and type /login".to_owned()
            } else {
                format!("Open Terminal and run {command}")
            };
            Failure {
                kind: FailureKind::NotSignedIn,
                message: format!("{name} is not signed in. {how}, then press Retry."),
                command: Some(command.to_owned()),
            }
        }
        AgentError::TimedOut { after } => failure(
            FailureKind::TimedOut,
            format!(
                "The agent did not finish within {} seconds, so it was stopped. Press Retry, \
                 or give it longer with agent.timeout_sec in the config file.",
                after.as_secs()
            ),
        ),
        AgentError::Cancelled => failure(
            FailureKind::Cancelled,
            "You cancelled the notes, so nothing was written.",
        ),
        AgentError::CliFailed { status, stderr } => {
            let said = short(stderr);
            let message = match (said.is_empty(), status) {
                (false, _) => format!("{name} stopped with an error: {said}"),
                (true, Some(code)) => {
                    format!("{name} stopped with an error (exit code {code}) and gave no reason.")
                }
                (true, None) => format!("{name} was stopped before it finished."),
            };
            failure(FailureKind::CliFailed, message)
        }
        AgentError::InvalidJson { .. } | AgentError::SchemaMismatch { .. } => bad_reply(),
        AgentError::CouldNotStart { reason } => could_not_start(reason),
    }
}

/// The reply was not notes JSON.
pub fn bad_reply() -> Failure {
    failure(
        FailureKind::BadReply,
        "The agent's answer did not match the notes format, so nothing was written. Press \
         Retry to ask again.",
    )
}

/// Something stopped the run before the agent got the transcript.
pub fn could_not_start(reason: impl std::fmt::Display) -> Failure {
    failure(
        FailureKind::CouldNotStart,
        format!("meet-ai could not start the notes: {reason}"),
    )
}

/// `agent.harness` is `none`.
pub fn no_agent() -> Failure {
    failure(
        FailureKind::NoAgent,
        "No agent is set up, so meet-ai cannot write the notes itself. Use Copy prompt, or \
         pick Claude Code in Settings.",
    )
}

/// The meeting has no transcript, or no folder at all.
pub fn no_transcript() -> Failure {
    failure(
        FailureKind::NoTranscript,
        "This meeting has no transcript yet, so there is nothing to write notes from.",
    )
}

/// The meeting is marked `agent_notes: off`.
pub fn notes_off() -> Failure {
    failure(
        FailureKind::NotesOff,
        "Notes are switched off for this meeting, so nothing was sent.",
    )
}

/// The notes came back but could not be saved.
pub fn write_failed(error: impl std::fmt::Display) -> Failure {
    failure(
        FailureKind::WriteFailed,
        format!("The notes came back but could not be saved: {error}"),
    )
}

/// `transcript.md` was still being written long after Stop, so no run was
/// started on half a transcript (TUR-17).
pub fn not_final() -> Failure {
    failure(
        FailureKind::CouldNotStart,
        "The transcript was still being saved, so no notes were written from part of it. \
         Press Retry once the transcript is complete.",
    )
}

/// A save refused before it ran (the meetings folder is moving) or with no
/// meetings root to save into.
impl From<UiError> for Failure {
    fn from(error: UiError) -> Self {
        write_failed(error.message)
    }
}

/// The worker thread died part-way.
pub fn crashed() -> Failure {
    failure(
        FailureKind::CouldNotStart,
        "The notes run stopped unexpectedly. Press Retry to try again.",
    )
}

/// `text` trimmed, and cut to its last [`STDERR_LIMIT`] characters, where the
/// actual error usually is.
fn short(text: &str) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= STDERR_LIMIT {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - STDERR_LIMIT).collect();
    format!("…{}", tail.trim_start())
}
