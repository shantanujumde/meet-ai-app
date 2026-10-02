//! Writing the agent's notes into a meeting folder (SPEC A11, TUR-7).
//!
//! After a call the user's own agent CLI answers with one JSON object, checked
//! against `prompts::notes::NOTES_SCHEMA` before it gets here. [`write`] turns
//! it into files:
//!
//! * the four fixed `meeting.md` sections (§3.2), plus `analyzed_by`,
//!   `analyzed_model` and `analyzed_at` in its frontmatter;
//! * one `tickets/TICK-NNNN.md` per task (§3.3), `status: open`, `assignee`
//!   from the task's owner, `transcript_ref` as given.
//!
//! Everything goes through [`Meeting`] and [`Ticket`], so the rules those
//! types keep apply here too: a `meeting.md` with broken frontmatter is
//! refused rather than overwritten (and then nothing at all is written), and
//! frontmatter keys this code does not know survive.
//!
//! **Ticket numbers are the app's**, not the prompt's: the highest `TICK-NNNN`
//! anywhere under the meetings root, plus one. [`write`] holds a process-wide
//! lock from that scan until its last file lands, so two notes runs finishing
//! together cannot hand out the same number.
//!
//! **Re-running notes** replaces the four sections. A ticket from an earlier
//! run is replaced only while the user has not touched it: still `open`, not
//! synced, and byte for byte the file the app wrote. To tell, `meeting.md`
//! keeps an `agent_tickets` map of ticket id → SHA-256 of the file as written.
//! A replaced ticket keeps its number. A ticket that fails the test is the
//! user's from then on: it is kept, and dropped from the map so no later run
//! claims it back. A ticket the user deleted stays deleted.
//!
//! **These are self-writes.** Every path written or removed is noted in the
//! [`SelfWrites`] passed in, as `notes.md` saves are, so the watcher does not
//! echo them back as outside changes.
//!
//! `notes.md` and `transcript.md` are never opened for writing here.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use prompts::notes::{Notes, Task};
use sha2::{Digest, Sha256};
use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use crate::folder::{self, meeting_dir};
use crate::folder_name::{prettify_slug, split_folder_name};
use crate::meeting::Meeting;
use crate::ticket::{self, Status, Ticket};
use crate::watcher::SelfWrites;
use crate::{Error, MEETING_FILE, TICKETS_DIR};

/// The `meeting.md` frontmatter key that records which tickets the app wrote,
/// and what each file held when it did.
pub const AGENT_TICKETS_KEY: &str = "agent_tickets";

/// Ticket frontmatter key for the task's due date, in the meeting's words
/// ("Friday"). Not a §3.3 key; unknown keys survive, so it rides along.
pub const DUE_KEY: &str = "due";

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

/// What [`write`] did with the tickets, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// `meeting.md`.
    pub meeting: PathBuf,
    /// One id per task, in task order: reused numbers first, then new ones.
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
/// * Whatever [`Meeting::render`] refuses with — broken frontmatter, or a
///   `meeting.md` that is not UTF-8. Checked before any file is written, so a
///   refused run leaves the folder exactly as it was.
/// * [`Error::Io`] when a file cannot be written. Tickets are written before
///   `meeting.md`, so a failure part-way leaves the old `meeting.md` (and its
///   record of which tickets are the app's) in place.
pub fn write(
    root: &Path,
    meeting_id: &str,
    notes: &Notes,
    analysis: &Analysis,
    self_writes: &SelfWrites,
) -> Result<Outcome, Error> {
    let _numbers = lock_numbers();

    let dir = meeting_dir(root, meeting_id)?;
    if !dir.is_dir() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("meeting folder {meeting_id} does not exist"),
        )));
    }
    let meeting_path = dir.join(MEETING_FILE);
    let mut meeting = match Meeting::read(&meeting_path)? {
        Some(meeting) => meeting,
        None => Meeting::new(meeting_id, &default_title(meeting_id)),
    };
    // Fail before any ticket lands, not at the last step.
    meeting.render()?;

    let tickets_dir = dir.join(TICKETS_DIR);
    let earlier = sort_earlier(&tickets_dir, &meeting);
    let mut next = highest_ticket_number(root)?.saturating_add(1);

    let mut record = Hash::new();
    let mut outcome = Outcome {
        meeting: meeting_path.clone(),
        kept: earlier.kept,
        ..Outcome::default()
    };
    let mut reusable = earlier.replaceable.into_iter();
    for task in &notes.tasks {
        let id = match reusable.next() {
            Some(id) => id,
            None => {
                let id = ticket::format_id(next);
                next = next.saturating_add(1);
                id
            }
        };
        let contents = ticket_for(&id, meeting_id, task).render()?;
        let path = tickets_dir.join(format!("{id}.md"));
        crate::write_atomic(&path, &contents)?;
        self_writes.note(&path);
        record.insert(Yaml::String(id.clone()), Yaml::String(digest(&contents)));
        outcome.written.push(id);
    }
    for id in reusable {
        let path = tickets_dir.join(format!("{id}.md"));
        // Note first: once the file is gone its path no longer resolves.
        self_writes.note(&path);
        match std::fs::remove_file(&path) {
            Ok(()) => outcome.removed.push(id),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let titles = folder::load(&dir)
        .map(|f| f.tickets)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| Some((t.id()?, t)))
        .collect::<Vec<_>>();
    fill_meeting(&mut meeting, notes, analysis, &outcome, &titles, record);
    meeting.write(&meeting_path)?;
    self_writes.note(&meeting_path);
    Ok(outcome)
}

fn lock_numbers() -> MutexGuard<'static, ()> {
    // The guard protects no data, so a panic elsewhere leaves nothing torn.
    TICKET_NUMBERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The title a brand-new `meeting.md` gets: the folder's slug, prettified, or
/// the id itself for a folder someone renamed by hand.
fn default_title(meeting_id: &str) -> String {
    match split_folder_name(meeting_id) {
        (_, _, Some(slug)) => prettify_slug(slug),
        _ => meeting_id.to_owned(),
    }
}

/// The tickets an earlier run wrote, split by whether this run may replace
/// them.
struct Earlier {
    /// Untouched: open, not synced, unchanged. In id order.
    replaceable: Vec<String>,
    /// Touched, so the user's now. In id order.
    kept: Vec<String>,
}

fn sort_earlier(tickets_dir: &Path, meeting: &Meeting) -> Earlier {
    let mut earlier = Earlier {
        replaceable: Vec::new(),
        kept: Vec::new(),
    };
    let Some(Yaml::Hash(record)) = meeting.frontmatter.get(AGENT_TICKETS_KEY) else {
        return earlier;
    };
    let mut ids: Vec<(String, String)> = record
        .iter()
        .filter_map(|(id, hash)| Some((id.as_str()?.to_owned(), hash.as_str()?.to_owned())))
        // A hand-edited record must not point outside the tickets folder.
        .filter(|(id, _)| ticket::parse_id(id).is_some())
        .collect();
    ids.sort_by_key(|(id, _)| ticket::parse_id(id));
    for (id, hash) in ids {
        let path = tickets_dir.join(format!("{id}.md"));
        match std::fs::read(&path) {
            Ok(bytes) if is_untouched(&bytes, &hash) => earlier.replaceable.push(id),
            Ok(_) => earlier.kept.push(id),
            // Deleted by the user (or unreadable): never recreated.
            Err(_) => {}
        }
    }
    earlier
}

/// Still `open`, not synced, and byte for byte what the app wrote.
fn is_untouched(bytes: &[u8], recorded: &str) -> bool {
    let Ok(raw) = std::str::from_utf8(bytes) else {
        return false;
    };
    if digest(raw) != recorded {
        return false;
    }
    let ticket = Ticket::parse(raw);
    ticket.status() == Some(Status::Open)
        && ticket.synced_to().is_none()
        && ticket.frontmatter.get_str("external_id").is_none()
        && ticket.frontmatter.get_str("external_url").is_none()
}

fn digest(contents: &str) -> String {
    format!("{:x}", Sha256::digest(contents.as_bytes()))
}

/// The highest ticket number anywhere under `root`: every meeting's
/// `tickets/` and the root's own `tickets/` (hand-made tickets, TUR-102).
///
/// Judged by file name, so a ticket with broken frontmatter still holds its
/// number.
fn highest_ticket_number(root: &Path) -> Result<u32, Error> {
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if let Some(n) = name.strip_suffix(".md").and_then(ticket::parse_id) {
                highest = highest.max(n);
            }
        }
    }
    Ok(highest)
}

fn ticket_for(id: &str, meeting_id: &str, task: &Task) -> Ticket {
    let mut made = Ticket::new(id, task.title.trim(), meeting_id);
    made.frontmatter.set_str("assignee", task.owner.as_deref());
    made.frontmatter
        .set_str("transcript_ref", Some(&task.transcript_ref));
    made.frontmatter.set_str(DUE_KEY, task.due.as_deref());
    let details = task.details.trim();
    made.body = if details.is_empty() {
        String::new()
    } else {
        format!("\n{details}\n")
    };
    made
}

fn fill_meeting(
    meeting: &mut Meeting,
    notes: &Notes,
    analysis: &Analysis,
    outcome: &Outcome,
    tickets: &[(String, Ticket)],
    record: Hash,
) {
    let fm = &mut meeting.frontmatter;
    fm.set_str("analyzed_by", Some(analysis.by.as_str()));
    fm.set_str("analyzed_model", Some(&analysis.model));
    fm.set_str("analyzed_at", Some(&analysis.at));
    if record.is_empty() {
        fm.remove(AGENT_TICKETS_KEY);
    } else {
        fm.set(AGENT_TICKETS_KEY, Yaml::Hash(record));
    }

    meeting.set_section("Summary", &paragraph(&notes.summary));
    meeting.set_section("Decisions", &bullets(&notes.decisions));
    let actions: Vec<String> = outcome
        .written
        .iter()
        .chain(&outcome.kept)
        .map(|id| action_line(id, tickets))
        .collect();
    meeting.set_section("Action Items", &bullets(&actions));
    meeting.set_section("Open Questions", &bullets(&notes.open_questions));
}

/// `TICK-0001 Move sessions to Redis (Priya, due Friday)`.
fn action_line(id: &str, tickets: &[(String, Ticket)]) -> String {
    let Some((_, ticket)) = tickets.iter().find(|(t, _)| t == id) else {
        return id.to_owned();
    };
    let mut line = format!("{id} {}", ticket.title().unwrap_or_default());
    let who = ticket.assignee();
    let due = ticket.frontmatter.get_str(DUE_KEY).map(|d| format!("due {d}"));
    let extra: Vec<String> = who.into_iter().chain(due).collect();
    if !extra.is_empty() {
        line.push_str(&format!(" ({})", extra.join(", ")));
    }
    line
}

/// Free text for a section body. A line that starts with `## ` would split
/// the section on the next read, so its `#` is escaped.
fn paragraph(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("## ") {
            out.push('\\');
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// One `- item` line per non-empty item, each folded onto a single line.
fn bullets(items: &[String]) -> String {
    let mut out = String::new();
    for item in items {
        let folded = item.split_whitespace().collect::<Vec<_>>().join(" ");
        if !folded.is_empty() {
            out.push_str("- ");
            out.push_str(&folded);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_in_the_summary_cannot_start_a_section() {
        assert_eq!(paragraph("ok\n## Evil\n"), "ok\n\\## Evil\n");
    }

    #[test]
    fn bullets_fold_lines_and_skip_blanks() {
        let items = ["one\ntwo".to_owned(), "  ".to_owned(), "three".to_owned()];
        assert_eq!(bullets(&items), "- one two\n- three\n");
    }

    #[test]
    fn analyzed_by_literals_match_the_spec() {
        assert_eq!(AnalyzedBy::ClaudeCode.as_str(), "claude-code");
        assert_eq!(AnalyzedBy::Codex.as_str(), "codex");
    }
}
