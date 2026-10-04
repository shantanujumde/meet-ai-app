//! Writing the agent's notes into a meeting folder (SPEC A11, TUR-7).
//!
//! After a call the user's own agent CLI answers with one JSON object, checked
//! against `prompts::notes::NOTES_SCHEMA` before it gets here. [`write()`] turns
//! it into files:
//!
//! * the four fixed `meeting.md` sections (§3.2), plus `analyzed_by`,
//!   `analyzed_model` and `analyzed_at` in its frontmatter;
//! * the agent's suggested `title`, unless the user named the meeting
//!   ([`crate::meeting_title`], TUR-103);
//! * one `tickets/TICK-NNNN.md` per task (§3.3), `status: open`, `assignee`
//!   from the task's owner, `transcript_ref` as given. §3.3 has no due date,
//!   so it goes in the body (`Due: Friday.`).
//!
//! The text matches what the copy-prompt fallback asks the agent to write
//! (`crates/prompts/templates/wrap-up.md`), so a meeting looks the same
//! whichever path made its notes. Action Items lists every ticket in the
//! meeting's folder, in number order, not only the ones this run wrote.
//!
//! Everything goes through [`Meeting`] and [`Ticket`], so the rules those
//! types keep apply here too: a `meeting.md` with broken frontmatter is
//! refused rather than overwritten (and then nothing at all is written), and
//! frontmatter keys this code does not know survive. A meeting marked
//! `agent_notes: off` is left alone.
//!
//! **Ticket numbers are the app's**, not the prompt's: the highest `TICK-NNNN`
//! anywhere under the meetings root, plus one. [`write()`] holds
//! [`lock_ticket_numbers`] from that scan until its last file lands, so two
//! notes runs finishing together cannot hand out the same number. Anything
//! else that numbers tickets should take the same lock.
//!
//! **Re-running notes** replaces the four sections. A ticket from an earlier
//! run is replaced only while the user has not touched it: still `open`, not
//! synced, and byte for byte the file the app wrote. To tell, `meeting.md`
//! keeps an `agent_tickets` map of ticket id → SHA-256 of the file as written.
//! A replaced ticket keeps its number, preferring the old ticket with the same
//! title. A ticket that fails the test is the user's from then on: it is kept,
//! and dropped from the map so no later run claims it back. A ticket the user
//! deleted stays deleted, and its number is not handed out again.
//!
//! **A run that stops part-way can be retried.** Before touching a ticket,
//! `meeting.md` is written with the map listing both the old and the new hash
//! of every ticket about to change, so a retry still recognises each one as
//! the app's whichever state it was left in. Only after the last ticket is
//! `meeting.md` written with its new sections. `meeting.md` is compared with
//! what was read before each write, so an edit made meanwhile is never
//! overwritten: the run stops instead.
//!
//! **These are self-writes.** Every path written or removed is noted in the
//! [`SelfWrites`] passed in, as `notes.md` saves are, so the watcher does not
//! echo them back as outside changes.
//!
//! `notes.md` and `transcript.md` are never opened for writing here.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use prompts::notes::{Notes, Task};
use sha2::{Digest, Sha256};
use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use crate::folder::{self, meeting_dir};
use crate::folder_name::{prettify_slug, split_folder_name};
use crate::meeting::Meeting;
use crate::meeting_title;
use crate::ticket::{self, Status, Ticket};
use crate::watcher::SelfWrites;
use crate::{Error, MEETING_FILE, TICKETS_DIR};

mod retired;
mod text;

pub use retired::highest_recorded_ticket_number;

/// The `meeting.md` frontmatter key that records which tickets the app wrote,
/// and what each file held when it did. Each value is one SHA-256 in hex, or a
/// list of them while a run is in flight.
pub const AGENT_TICKETS_KEY: &str = "agent_tickets";

/// The `meeting.md` key (SPEC A11) by which the user keeps a meeting's
/// transcript from being sent. `off` means no notes run.
pub const AGENT_NOTES_KEY: &str = "agent_notes";

/// Held from the ticket-number scan to the last write. Process-wide: the
/// meetings root is one per app, and only this process allocates numbers.
static TICKET_NUMBERS: Mutex<()> = Mutex::new(());

/// Which CLI produced the notes. Written as `analyzed_by`.
///
/// `clipboard` (the copy-prompt fallback, §3.2) is not here: on that path the
/// agent writes the files itself and this module is not involved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzedBy {
    ClaudeCode,
    Codex,
}

impl AnalyzedBy {
    /// The literal written to `analyzed_by`: `claude-code` or `codex`.
    pub fn as_str(self) -> &'static str {
        match self {
            AnalyzedBy::ClaudeCode => "claude-code",
            AnalyzedBy::Codex => "codex",
        }
    }
}

/// Who wrote the notes, with what, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub by: AnalyzedBy,
    /// The model name the CLI ran, as configured (`opus`).
    pub model: String,
    /// ISO-8601 with offset, e.g. `2026-09-01T15:32:00+05:30`. The caller
    /// supplies it so this crate stays free of a clock.
    pub at: String,
}

/// What [`write()`] did with the tickets, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// `meeting.md`.
    pub meeting: PathBuf,
    /// The meeting is marked `agent_notes: off`, so nothing was written.
    pub notes_off: bool,
    /// One id per task, in task order.
    pub written: Vec<String>,
    /// Tickets from an earlier run the user had touched, left as they were.
    pub kept: Vec<String>,
    /// Untouched tickets from an earlier run with no task left to take their
    /// number, deleted.
    pub removed: Vec<String>,
}

/// Write `notes` into meeting `meeting_id` under `root`. See the module docs.
///
/// # Errors
///
/// * [`Error::BadId`] for a meeting id that is not a plain folder name.
/// * [`Error::Io`] of kind `NotFound` when the meeting folder does not exist.
/// * [`Error::Frontmatter`] for a `meeting.md` with broken frontmatter, and
///   [`Error::Io`] of kind `InvalidData` for one that is not UTF-8. Both are
///   checked before any file is written, so the folder is left as it was.
/// * [`Error::Io`] of kind `Other` when `meeting.md` changed on disk while
///   the run was writing. What was written so far is safe to retry over.
/// * [`Error::Io`] when a file cannot be read or written.
pub fn write(
    root: &Path,
    meeting_id: &str,
    notes: &Notes,
    analysis: &Analysis,
    self_writes: &SelfWrites,
) -> Result<Outcome, Error> {
    let _numbers = lock_ticket_numbers();

    let dir = meeting_dir(root, meeting_id)?;
    if !dir.is_dir() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("meeting folder {meeting_id} does not exist"),
        )));
    }
    let meeting_path = dir.join(MEETING_FILE);
    let mut on_disk = read_if_there(&meeting_path)?;
    let mut meeting = match &on_disk {
        Some(bytes) => Meeting::parse(std::str::from_utf8(bytes).map_err(|error| {
            Error::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{MEETING_FILE} is not UTF-8 ({error}); refusing to overwrite it"),
            ))
        })?),
        None => Meeting::new(meeting_id, &default_title(meeting_id)),
    };
    let mut outcome = Outcome {
        meeting: meeting_path.clone(),
        ..Outcome::default()
    };
    if notes_are_off(&meeting) {
        outcome.notes_off = true;
        return Ok(outcome);
    }
    // Fail before any ticket lands, not at the last step.
    meeting.render()?;

    let tickets_dir = dir.join(TICKETS_DIR);
    let earlier = sort_earlier(&tickets_dir, &meeting);
    // A number the user's deletion freed stays retired: counting the record
    // too keeps a deleted top ticket from coming back under its old name.
    let next = highest_ticket_number(root)?
        .max(earlier.highest)
        .saturating_add(1);
    let plan = plan(
        &tickets_dir,
        meeting_id,
        &notes.tasks,
        earlier.replaceable,
        next,
    )?;
    outcome.kept = earlier.kept;

    // Step 1: say which tickets are about to change, in both their states.
    if !plan.writes.is_empty() || !plan.leftovers.is_empty() {
        let mut intent = earlier.carried.clone();
        for planned in &plan.writes {
            let hashes = planned.old_hash.iter().chain([&planned.new_hash]);
            intent.insert(key(&planned.id), hash_list(hashes));
        }
        for leftover in &plan.leftovers {
            intent.insert(key(&leftover.id), hash_list([&leftover.hash]));
        }
        let mut staged = meeting.clone();
        set_record(&mut staged, intent);
        on_disk = save_meeting(&meeting_path, &staged, on_disk.as_deref(), self_writes)?;
    }

    // Step 2: the tickets.
    if !plan.writes.is_empty() && !tickets_dir.is_dir() {
        std::fs::create_dir_all(&tickets_dir)?;
        self_writes.note(&tickets_dir);
    }
    let mut record = earlier.carried;
    for planned in &plan.writes {
        crate::write_atomic(&planned.path, &planned.contents)?;
        self_writes.note(&planned.path);
        record.insert(key(&planned.id), Yaml::String(planned.new_hash.clone()));
        outcome.written.push(planned.id.clone());
    }
    for leftover in plan.leftovers {
        let path = ticket_path(&tickets_dir, &leftover.id);
        // Note first: once the file is gone its path no longer resolves.
        self_writes.note(&path);
        match std::fs::remove_file(&path) {
            Ok(()) => outcome.removed.push(leftover.id),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    // Step 3: the title, the sections, and the record as it now stands.
    meeting_title::suggest(&mut meeting, &notes.title, &default_title(meeting_id));
    for (k, v) in [
        ("analyzed_by", analysis.by.as_str()),
        ("analyzed_model", analysis.model.as_str()),
        ("analyzed_at", analysis.at.as_str()),
    ] {
        meeting.frontmatter.set_str(k, Some(v));
    }
    set_record(&mut meeting, record);
    let actions = action_items(&tickets_dir, &plan.writes);
    text::fill_sections(&mut meeting, notes, &actions);
    save_meeting(&meeting_path, &meeting, on_disk.as_deref(), self_writes)?;
    Ok(outcome)
}

/// The lock every writer of a new ticket number should hold from choosing the
/// number until the file is on disk.
///
/// The guard protects no data, so a panic elsewhere leaves nothing torn and a
/// poisoned lock is taken as is.
pub fn lock_ticket_numbers() -> MutexGuard<'static, ()> {
    TICKET_NUMBERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The highest ticket number anywhere under `root`: every meeting's
/// `tickets/` and the root's own `tickets/` (hand-made tickets, TUR-102).
///
/// Judged by file name, so a ticket with broken frontmatter still holds its
/// number. A `tickets/` that cannot be listed is logged and skipped rather
/// than failing every meeting's notes for one broken folder (SPEC §7); the
/// writer still refuses to replace a file that exists.
///
/// # Errors
///
/// [`Error::Io`] when `root` itself cannot be listed.
pub fn highest_ticket_number(root: &Path) -> Result<u32, Error> {
    let mut dirs = vec![root.join(TICKETS_DIR)];
    dirs.extend(
        folder::meeting_dirs(root)?
            .into_iter()
            .map(|dir| dir.join(TICKETS_DIR)),
    );
    let mut highest = 0;
    for dir in dirs {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                tracing::warn!(path = %dir.display(), %error, "skipping a tickets folder that could not be listed");
                continue;
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(n) = name
                .to_string_lossy()
                .strip_suffix(".md")
                .and_then(ticket::parse_id)
            {
                highest = highest.max(n);
            }
        }
    }
    Ok(highest)
}

fn read_if_there(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Write `meeting` over `path`, but only if the file still holds `expected`
/// (`None`: still absent). Returns what is on disk now.
fn save_meeting(
    path: &Path,
    meeting: &Meeting,
    expected: Option<&[u8]>,
    self_writes: &SelfWrites,
) -> Result<Option<Vec<u8>>, Error> {
    if read_if_there(path)?.as_deref() != expected {
        return Err(Error::Io(io::Error::other(format!(
            "{MEETING_FILE} changed while the notes were being written; \
             it was left as it is. Run the notes again."
        ))));
    }
    let contents = meeting.render()?;
    crate::write_atomic(path, &contents)?;
    self_writes.note(path);
    Ok(Some(contents.into_bytes()))
}

pub(crate) fn notes_are_off(meeting: &Meeting) -> bool {
    matches!(
        meeting.frontmatter.get_str(AGENT_NOTES_KEY).as_deref(),
        Some(crate::notes_switch::OFF | "false")
    )
}

/// The title a brand-new `meeting.md` gets: the folder's slug, prettified, or
/// the id itself for a folder someone renamed by hand.
pub(crate) fn default_title(meeting_id: &str) -> String {
    match split_folder_name(meeting_id) {
        (_, _, Some(slug)) => prettify_slug(slug),
        _ => meeting_id.to_owned(),
    }
}

fn ticket_path(tickets_dir: &Path, id: &str) -> PathBuf {
    tickets_dir.join(format!("{id}.md"))
}

fn key(id: &str) -> Yaml {
    Yaml::String(id.to_owned())
}

fn hash_list<'a>(hashes: impl IntoIterator<Item = &'a String>) -> Yaml {
    Yaml::Array(hashes.into_iter().cloned().map(Yaml::String).collect())
}

fn set_record(meeting: &mut Meeting, record: Hash) {
    if record.is_empty() {
        meeting.frontmatter.remove(AGENT_TICKETS_KEY);
    } else {
        meeting
            .frontmatter
            .set(AGENT_TICKETS_KEY, Yaml::Hash(record));
    }
}

/// An untouched ticket from an earlier run.
struct Prior {
    id: String,
    title: Option<String>,
    hash: String,
}

/// The tickets an earlier run wrote, sorted by what this run may do with them.
struct Earlier {
    /// Untouched: open, not synced, unchanged. In id order.
    replaceable: Vec<Prior>,
    /// Touched, so the user's now. In id order.
    kept: Vec<String>,
    /// Record entries kept as they are: tickets that could not be read just
    /// now, so neither touched nor deleted can be told.
    carried: Hash,
    /// The highest number in the record, deleted tickets included.
    highest: u32,
}

fn sort_earlier(tickets_dir: &Path, meeting: &Meeting) -> Earlier {
    let mut earlier = Earlier {
        replaceable: Vec::new(),
        kept: Vec::new(),
        carried: Hash::new(),
        highest: 0,
    };
    let Some(Yaml::Hash(record)) = meeting.frontmatter.get(AGENT_TICKETS_KEY) else {
        return earlier;
    };
    let mut entries: Vec<(u32, String, &Yaml)> = record
        .iter()
        .filter_map(|(id, hashes)| {
            let id = id.as_str()?;
            // A hand-edited record must not point outside the tickets folder.
            Some((ticket::parse_id(id)?, id.to_owned(), hashes))
        })
        .collect();
    entries.sort_by_key(|(n, _, _)| *n);
    earlier.highest = entries.last().map_or(0, |(n, _, _)| *n);
    for (_, id, hashes) in entries {
        let recorded: Vec<&str> = match hashes {
            Yaml::String(one) => vec![one.as_str()],
            Yaml::Array(many) => many.iter().filter_map(Yaml::as_str).collect(),
            _ => Vec::new(),
        };
        match std::fs::read(ticket_path(tickets_dir, &id)) {
            Ok(bytes) => match untouched(&bytes, &recorded) {
                Some((hash, title)) => earlier.replaceable.push(Prior { id, title, hash }),
                None => earlier.kept.push(id),
            },
            // Deleted by the user: never recreated.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%id, %error, "could not read a ticket the app wrote; leaving it be");
                earlier.carried.insert(key(&id), hashes.clone());
            }
        }
    }
    earlier
}

/// Still `open`, not synced, and byte for byte what the app wrote: its hash
/// and title, or `None` if the user has touched it.
fn untouched(bytes: &[u8], recorded: &[&str]) -> Option<(String, Option<String>)> {
    let raw = std::str::from_utf8(bytes).ok()?;
    let hash = digest(raw);
    if !recorded.contains(&hash.as_str()) {
        return None;
    }
    let ticket = Ticket::parse(raw);
    let unsynced = ["synced_to", "external_id", "external_url"]
        .iter()
        .all(|k| ticket.frontmatter.get_str(k).is_none());
    (ticket.status() == Some(Status::Open) && unsynced).then(|| (hash, ticket.title()))
}

fn digest(contents: &str) -> String {
    format!("{:x}", Sha256::digest(contents.as_bytes()))
}

/// One ticket this run will write.
struct Planned<'a> {
    id: String,
    path: PathBuf,
    task: &'a Task,
    contents: String,
    new_hash: String,
    /// The replaced ticket's hash; `None` for a new number.
    old_hash: Option<String>,
}

struct Plan<'a> {
    /// In task order.
    writes: Vec<Planned<'a>>,
    /// Untouched tickets no task took: deleted.
    leftovers: Vec<Prior>,
}

/// Give every task an id. An untouched ticket with the same title is reused
/// first, so a ticket people already refer to keeps its number when the task
/// comes back; then the rest of the untouched ones in id order; then new
/// numbers from `next`, skipping any file that already exists.
fn plan<'a>(
    tickets_dir: &Path,
    meeting_id: &str,
    tasks: &'a [Task],
    mut free: Vec<Prior>,
    mut next: u32,
) -> Result<Plan<'a>, Error> {
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let mut chosen: Vec<Option<Prior>> = tasks
        .iter()
        .map(|task| {
            // The title as it was written to the file, not as sent.
            let title = text::clean(task).title;
            let i = free
                .iter()
                .position(|p| p.title.as_deref().is_some_and(|t| same(t, &title)))?;
            Some(free.remove(i))
        })
        .collect();
    let mut by_order = free.into_iter();
    for slot in chosen.iter_mut().filter(|slot| slot.is_none()) {
        *slot = by_order.next();
    }
    let leftovers = by_order.collect();

    let mut taken: HashSet<String> = HashSet::new();
    let mut writes = Vec::with_capacity(tasks.len());
    for (task, prior) in tasks.iter().zip(chosen) {
        let (id, old_hash) = match prior {
            Some(prior) => (prior.id, Some(prior.hash)),
            None => loop {
                let id = ticket::format_id(next);
                next = next.saturating_add(1);
                if !ticket_path(tickets_dir, &id).exists() && taken.insert(id.clone()) {
                    break (id, None);
                }
            },
        };
        let mut made = text::ticket_for(&id, meeting_id, task);
        made.frontmatter
            .set_str("transcript_ref", Some(&task.transcript_ref));
        let contents = made.render()?;
        writes.push(Planned {
            path: ticket_path(tickets_dir, &id),
            new_hash: digest(&contents),
            id,
            task,
            contents,
            old_hash,
        });
    }
    Ok(Plan { writes, leftovers })
}

/// One line per ticket in the meeting's folder, in number order. The tickets
/// this run wrote use the task's words; the rest are read back from disk.
fn action_items(tickets_dir: &Path, writes: &[Planned<'_>]) -> Vec<String> {
    let mut ids: Vec<(u32, String)> = std::fs::read_dir(tickets_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let id = name.strip_suffix(".md")?.to_owned();
                    Some((ticket::parse_id(&id)?, id))
                })
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.into_iter()
        .map(|(_, id)| {
            if let Some(planned) = writes.iter().find(|p| p.id == id) {
                let task = text::clean(planned.task);
                return text::action_line(
                    &id,
                    &task.title,
                    task.owner.as_deref(),
                    task.due.as_deref(),
                );
            }
            let found = Ticket::read(&ticket_path(tickets_dir, &id)).ok();
            let title = found.as_ref().and_then(Ticket::title).unwrap_or_default();
            let owner = found.as_ref().and_then(Ticket::assignee);
            let due = found.as_ref().and_then(|t| text::due_in(&t.body));
            text::action_line(&id, &title, owner.as_deref(), due.as_deref())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzed_by_literals_match_the_spec() {
        assert_eq!(AnalyzedBy::ClaudeCode.as_str(), "claude-code");
        assert_eq!(AnalyzedBy::Codex.as_str(), "codex");
    }

    #[test]
    fn a_ticket_matching_either_recorded_hash_is_untouched() {
        let raw = Ticket::new("TICK-0001", "Ship", "m").render().unwrap();
        let hash = digest(&raw);
        assert!(untouched(raw.as_bytes(), &["old", &hash]).is_some());
        assert!(untouched(raw.as_bytes(), &["old"]).is_none());
    }
}
