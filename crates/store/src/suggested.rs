//! Suggested tasks: the tickets a notes run finds in a meeting, which the user
//! approves into Tickets or discards (SPEC A25, TUR-113).
//!
//! * **Suggested** is a file in the meeting's own `tickets/` folder, where
//!   [`crate::agent_notes::write()`] puts it.
//! * **Approved** is the same file moved to the root's `tickets/` folder,
//!   beside the hand-made tickets, keeping its `id` and `meeting:`. A missing
//!   `meeting:` or `status:` is filled in on the way (`status: open`). The
//!   meeting still lists it, because it names the meeting.
//! * **Discarded** is the file deleted. Its id goes on the meeting's
//!   `retired_tickets` list, so no writer hands the number out again, and its
//!   line leaves the meeting's Action Items.
//!
//! Every change holds [`lock_meeting_writers`], so it never lands in the
//! middle of a notes run numbering or rewriting the same tickets. A notes run
//! does not bring an approved or discarded task back: to it, the file is gone
//! from the meeting's folder, as one the user deleted is.
//!
//! As in [`crate::agent_notes`], every path written, moved or removed is noted
//! in the [`SelfWrites`] passed in.

use std::io;
use std::path::{Path, PathBuf};

use crate::agent_notes::{default_title, lock_meeting_writers, retire};
use crate::folder::{self, meeting_dir};
use crate::meeting::Meeting;
use crate::ticket::{self, Status, Ticket};
use crate::watcher::SelfWrites;
use crate::{Error, MEETING_FILE, TICKETS_DIR};

/// The Action Items heading [`discard`] takes a line out of.
const ACTION_ITEMS: &str = "Action Items";

/// What [`approve_all`] did, by ticket id, in number order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovedAll {
    /// Moved to the root's `tickets/` folder.
    pub approved: Vec<String>,
    /// Left as suggestions: the file could not be moved (broken frontmatter,
    /// an id already taken in the root). Each one is logged.
    pub skipped: Vec<String>,
}

/// One ticket moved by [`approve_synced`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub meeting_id: String,
    pub ticket_id: String,
}

/// The suggested tasks of meeting `meeting_id`: the ticket ids in its own
/// `tickets/` folder, in number order. A missing folder has none.
///
/// # Errors
///
/// [`Error::BadId`] for a meeting id that is not a plain folder name, and
/// [`Error::Io`] when the folder cannot be listed.
pub fn suggested(root: &Path, meeting_id: &str) -> Result<Vec<String>, Error> {
    let dir = meeting_dir(root, meeting_id)?.join(TICKETS_DIR);
    ticket_ids(&dir)
}

/// Approve one suggested task: move `<meeting>/tickets/<id>.md` to
/// `<root>/tickets/<id>.md`. Returns the new path.
///
/// # Errors
///
/// * [`Error::BadId`] for a meeting or ticket id that is not a plain name.
/// * [`Error::Io`] of kind `NotFound` when the meeting has no such suggestion,
///   and of kind `AlreadyExists` when the root already has a ticket with that
///   id. Neither file is touched.
/// * [`Error::Frontmatter`] when the file has broken frontmatter and so cannot
///   get its `meeting:` and `status:` written. It stays a suggestion.
/// * [`Error::Io`] when a file cannot be read, written or moved.
pub fn approve(
    root: &Path,
    meeting_id: &str,
    ticket_id: &str,
    self_writes: &SelfWrites,
) -> Result<PathBuf, Error> {
    let _writers = lock_meeting_writers();
    move_to_tickets(root, meeting_id, ticket_id, self_writes)
}

/// Approve every suggested task of meeting `meeting_id`, in number order. A
/// task that cannot be moved is skipped and stays a suggestion; the others
/// still move.
///
/// # Errors
///
/// [`Error::BadId`] for a meeting id that is not a plain folder name, and
/// [`Error::Io`] when the meeting's `tickets/` folder cannot be listed.
pub fn approve_all(
    root: &Path,
    meeting_id: &str,
    self_writes: &SelfWrites,
) -> Result<ApprovedAll, Error> {
    let _writers = lock_meeting_writers();
    let mut outcome = ApprovedAll::default();
    for id in suggested(root, meeting_id)? {
        match move_to_tickets(root, meeting_id, &id, self_writes) {
            Ok(_) => outcome.approved.push(id),
            Err(error) => {
                tracing::warn!(meeting_id, ticket_id = %id, %error, "could not approve a suggested task; left it as a suggestion");
                outcome.skipped.push(id);
            }
        }
    }
    Ok(outcome)
}

/// Discard one suggested task: delete its file, add its id to the meeting's
/// `retired_tickets`, and take its line out of the meeting's Action Items.
/// `meeting.md` is written first, so a failure leaves the task in place.
///
/// # Errors
///
/// * [`Error::BadId`] for a meeting or ticket id that is not a plain name.
/// * [`Error::Io`] of kind `NotFound` when the meeting has no such suggestion.
/// * [`Error::Frontmatter`] for a `meeting.md` with broken frontmatter, and
///   [`Error::Io`] of kind `InvalidData` for one that is not UTF-8. Nothing
///   is deleted then: the number could not be retired.
/// * [`Error::Io`] when a file cannot be read, written or removed.
pub fn discard(
    root: &Path,
    meeting_id: &str,
    ticket_id: &str,
    self_writes: &SelfWrites,
) -> Result<(), Error> {
    let _writers = lock_meeting_writers();
    let path = suggestion_path(root, meeting_id, ticket_id)?;
    if !path.is_file() {
        return Err(not_a_suggestion(meeting_id, ticket_id));
    }
    let meeting_path = meeting_dir(root, meeting_id)?.join(MEETING_FILE);
    let mut meeting = match Meeting::read(&meeting_path)? {
        Some(meeting) => {
            // Refuse before changing anything, as the notes switch does.
            meeting.render()?;
            meeting
        }
        None => Meeting::new(meeting_id, &default_title(meeting_id)),
    };
    retire(&mut meeting, ticket_id);
    drop_action_line(&mut meeting, ticket_id);
    meeting.write(&meeting_path)?;
    self_writes.note(&meeting_path);

    // Note first: once the file is gone its path no longer resolves.
    self_writes.note(&path);
    std::fs::remove_file(&path)?;
    Ok(())
}

/// The update step for tickets from before suggestions (SPEC A25): every
/// ticket in a meeting's folder that was already synced to a tracker counts
/// as approved, so it moves to the root's `tickets/`. The rest stay
/// suggestions. Running it again finds nothing more to move.
///
/// A ticket that cannot be read or moved is logged and left where it is.
///
/// # Errors
///
/// [`Error::Io`] when `root` itself cannot be listed.
pub fn approve_synced(root: &Path, self_writes: &SelfWrites) -> Result<Vec<Moved>, Error> {
    let _writers = lock_meeting_writers();
    let mut moved = Vec::new();
    let mut dirs = folder::meeting_dirs(root)?;
    dirs.sort();
    for dir in dirs {
        let Some(meeting_id) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let ids = match ticket_ids(&dir.join(TICKETS_DIR)) {
            Ok(ids) => ids,
            Err(error) => {
                tracing::warn!(meeting_id, %error, "could not list a meeting's tickets; skipping it");
                continue;
            }
        };
        for ticket_id in ids {
            let path = suggestion_path(root, meeting_id, &ticket_id)?;
            let synced = Ticket::read(&path)
                .ok()
                .and_then(|t| t.synced_to())
                .is_some_and(|s| !s.trim().is_empty());
            if !synced {
                continue;
            }
            match move_to_tickets(root, meeting_id, &ticket_id, self_writes) {
                Ok(_) => moved.push(Moved {
                    meeting_id: meeting_id.to_owned(),
                    ticket_id,
                }),
                Err(error) => {
                    tracing::warn!(meeting_id, %ticket_id, %error, "could not move a synced ticket to Tickets; left it in its meeting");
                }
            }
        }
    }
    Ok(moved)
}

/// [`approve`] with the lock already held.
fn move_to_tickets(
    root: &Path,
    meeting_id: &str,
    ticket_id: &str,
    self_writes: &SelfWrites,
) -> Result<PathBuf, Error> {
    let from = suggestion_path(root, meeting_id, ticket_id)?;
    let mut found = match Ticket::read(&from) {
        Ok(found) => found,
        Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Err(not_a_suggestion(meeting_id, ticket_id));
        }
        Err(error) => return Err(error),
    };
    let to_dir = root.join(TICKETS_DIR);
    let to = to_dir.join(file_name(ticket_id));
    if to.exists() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Tickets already has a {ticket_id}; {ticket_id} was left in its meeting"),
        )));
    }

    // Fill in what the root's copy needs to still name its meeting.
    let mut changed = false;
    if found.id().is_none() {
        found.frontmatter.set_str("id", Some(ticket_id));
        changed = true;
    }
    if found.meeting().is_none_or(|m| m.trim().is_empty()) {
        found.frontmatter.set_str("meeting", Some(meeting_id));
        changed = true;
    }
    if found.status().is_none() {
        found.set_status(Status::Open);
        changed = true;
    }
    if changed {
        found.write(&from)?;
        self_writes.note(&from);
    }

    if !to_dir.is_dir() {
        std::fs::create_dir_all(&to_dir)?;
        self_writes.note(&to_dir);
    }
    // Note first: once the file is gone its path no longer resolves.
    self_writes.note(&from);
    // One rename on the same disk: the ticket is in one folder or the other,
    // never both or neither.
    std::fs::rename(&from, &to)?;
    self_writes.note(&to);
    Ok(to)
}

/// `<root>/<meeting>/tickets/<id>.md`, refusing ids that are not plain names
/// or not `TICK-NNNN`.
fn suggestion_path(root: &Path, meeting_id: &str, ticket_id: &str) -> Result<PathBuf, Error> {
    if ticket::parse_id(ticket_id).is_none() {
        return Err(Error::BadId(ticket_id.to_owned()));
    }
    Ok(meeting_dir(root, meeting_id)?
        .join(TICKETS_DIR)
        .join(file_name(ticket_id)))
}

fn file_name(ticket_id: &str) -> String {
    format!("{ticket_id}.md")
}

fn not_a_suggestion(meeting_id: &str, ticket_id: &str) -> Error {
    Error::Io(io::Error::new(
        io::ErrorKind::NotFound,
        format!("{ticket_id} is not a suggested task of {meeting_id}"),
    ))
}

/// The `TICK-NNNN.md` file stems in `dir`, in number order. A missing folder
/// has none.
fn ticket_ids(dir: &Path) -> Result<Vec<String>, Error> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut ids: Vec<(u32, String)> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".md")?.to_owned();
            Some((ticket::parse_id(&id)?, id))
        })
        .collect();
    ids.sort();
    Ok(ids.into_iter().map(|(_, id)| id).collect())
}

/// Take `ticket_id`'s bullet (`- TICK-0003: ...`) out of the Action Items
/// section, writing "None." when no bullet is left. A meeting with no such
/// section, or no such line, is left as it is.
fn drop_action_line(meeting: &mut Meeting, ticket_id: &str) {
    let Some(body) = meeting.section(ACTION_ITEMS) else {
        return;
    };
    let line_of = format!("- {ticket_id}:");
    let mut dropped = false;
    let kept: Vec<&str> = body
        .lines()
        .filter(|line| {
            let mine = line.trim_start().starts_with(&line_of);
            dropped |= mine;
            !mine
        })
        .collect();
    if !dropped {
        return;
    }
    let eol = meeting.eol();
    let any_bullet = kept.iter().any(|line| line.trim_start().starts_with("- "));
    let new_body = if any_bullet {
        format!("{}{eol}", kept.join(eol).trim_end())
    } else {
        format!("None.{eol}")
    };
    meeting.set_section(ACTION_ITEMS, &new_body);
}
