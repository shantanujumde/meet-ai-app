//! The two prompts the window copies to the clipboard (SPEC L14, A11).
//!
//! **Start Work** on a ticket: `start-work.md`, filled with the ticket, the
//! transcript lines around its `transcript_ref`, and the repo to work in.
//! Starting work needs the user's own interactive agent session in that repo,
//! so this stays a copied prompt even when the app can run the agent.
//!
//! **Copy prompt** on a meeting, the fallback when no agent is set up: the
//! clipboard version of `wrap-up.md`. The agent the user pastes it into writes
//! `meeting.md` and the ticket files itself, and the folder watcher picks them
//! up (the original L9 path, `analyzed_by: clipboard`).
//!
//! Both only read. The webview does the copying, so a refused clipboard is the
//! window's problem to show, not an error here.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use prompts::start_work::{StartWorkInput, render_start_work_from};
use prompts::wrap_up::{Target, WrapUpInput, render_wrap_up_from};
use store::meeting::Meeting;
use store::ticket::Ticket;

use crate::config::{self, Harness};
use crate::error::UiError;
use crate::meetings;

impl From<prompts::Error> for UiError {
    fn from(error: prompts::Error) -> Self {
        let kind = match error {
            prompts::Error::Template { .. } => "prompt-template",
            prompts::Error::Io(_) => "io",
            prompts::Error::InvalidNotes { .. } => "invalid-notes",
        };
        Self::app(kind, error.to_string())
    }
}

/// The Start Work prompt for one ticket. `meeting_id` is the meeting the
/// window listed it under, if any; the ticket's own `meeting` key is used
/// otherwise.
pub fn start_work(ticket_id: &str, meeting_id: Option<&str>) -> Result<String, UiError> {
    start_work_in(
        &meetings::root()?,
        ticket_id,
        meeting_id,
        config::default_repo()?,
    )
}

/// Should the meeting view offer Copy prompt? True when `agent.harness` is
/// `none` (A11). Whether the chosen CLI is installed is the window's other
/// half of the rule, filled in once agent CLI detection lands (TUR-6).
pub fn fallback() -> Result<bool, UiError> {
    Ok(config::agent()?.harness == Harness::None)
}

/// The clipboard wrap-up prompt for one meeting.
pub fn wrap_up(meeting_id: &str) -> Result<String, UiError> {
    wrap_up_in(&meetings::root()?, meeting_id)
}

/// [`start_work`] under `root`. `default_repo` is the repo to name when the
/// meeting links none.
fn start_work_in(
    root: &Path,
    ticket_id: &str,
    meeting_id: Option<&str>,
    default_repo: Option<String>,
) -> Result<String, UiError> {
    let ticket_file = find_ticket(root, ticket_id, meeting_id)?;
    let found = Ticket::read(&ticket_file)?;
    let meeting_id = meeting_id
        .map(str::to_owned)
        .or_else(|| found.meeting())
        .filter(|id| !id.is_empty());

    let mut meeting_title = None;
    let mut repo = None;
    let mut excerpt = String::new();
    if let Some(id) = &meeting_id {
        let dir = store::folder::meeting_dir(root, id)?;
        let meeting = read_meeting(&dir)?;
        meeting_title = Some(store::meeting::display_title(meeting.as_ref(), id));
        repo = meeting.and_then(|meeting| meeting.frontmatter.get_str("repo"));
        if let Some(at) = found.transcript_ref() {
            let transcript = read_optional(&dir.join(store::TRANSCRIPT_FILE))?;
            excerpt = prompts::transcript_excerpt(&transcript, &at).unwrap_or_default();
        }
    }

    let input = StartWorkInput {
        ticket_id: found.id().unwrap_or_else(|| ticket_id.to_owned()),
        title: found.title().unwrap_or_else(|| ticket_id.to_owned()),
        details: found.body.trim().to_owned(),
        assignee: found.assignee(),
        transcript_ref: found.transcript_ref(),
        excerpt,
        meeting_id,
        meeting_title,
        repo: repo.or(default_repo).filter(|path| !path.trim().is_empty()),
        ticket_file: Some(ticket_file.display().to_string()),
    };
    Ok(render_start_work_from(root, &input)?)
}

/// [`wrap_up`] under `root`.
fn wrap_up_in(root: &Path, meeting_id: &str) -> Result<String, UiError> {
    let dir = store::folder::meeting_dir(root, meeting_id)?;
    if !dir.is_dir() {
        return Err(UiError::app(
            "meeting-not-found",
            format!("There is no meeting folder called {meeting_id}."),
        ));
    }
    let transcript = read_optional(&dir.join(store::TRANSCRIPT_FILE))?;
    if transcript.trim().is_empty() {
        return Err(UiError::app(
            "transcript-empty",
            "This meeting has no transcript yet, so there is nothing to write notes from.",
        ));
    }
    let meeting = read_meeting(&dir)?;
    let input = WrapUpInput {
        title: store::meeting::display_title(meeting.as_ref(), meeting_id),
        date: store::meeting::display_date(meeting.as_ref(), meeting_id).unwrap_or_default(),
        transcript,
        notes: store::notes::read(&dir)?,
    };
    let target = Target::Clipboard {
        first_ticket: next_ticket_number(root)?,
        meeting_dir: dir,
    };
    Ok(render_wrap_up_from(root, &input, &target)?)
}

/// The ticket's file ([`crate::tickets::find`]). Any plain file name may be
/// asked for; one that is not there, or not a plain name, is
/// `ticket-not-found`.
fn find_ticket(root: &Path, ticket_id: &str, meeting_id: Option<&str>) -> Result<PathBuf, UiError> {
    crate::tickets::find(root, ticket_id, meeting_id)?.ok_or_else(|| {
        UiError::app(
            "ticket-not-found",
            format!("Could not find the file for ticket {ticket_id}."),
        )
    })
}

/// The first number the pasted agent may use: the same global rule as a notes
/// run and a hand-made ticket (TUR-154), so its files never take a number any
/// meeting already uses or retired.
fn next_ticket_number(root: &Path) -> Result<u32, UiError> {
    Ok(store::agent_notes::next_ticket_number(root)?)
}

/// `meeting.md`, or `None` when the meeting has not been wrapped up yet.
fn read_meeting(dir: &Path) -> Result<Option<Meeting>, UiError> {
    Ok(Meeting::read(&dir.join(store::MEETING_FILE))?)
}

/// A text file's contents, or empty when it does not exist.
fn read_optional(path: &Path) -> Result<String, UiError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
