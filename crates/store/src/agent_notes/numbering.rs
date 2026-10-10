//! The one rule every writer of a new ticket number follows (TUR-154): a
//! notes run, a hand-made ticket and the copied wrap-up prompt all number
//! tickets for the whole meetings root, never per meeting, and never reuse a
//! number any meeting still records or retired.
//!
//! Reading that from every meeting (each `tickets/` listed, each `meeting.md`
//! parsed) on every new ticket grew with the archive, under the lock that
//! every ticket writer waits on. [`next_ticket_number`] keeps each meeting's
//! highest number and reads a meeting again only when its `meeting.md` or
//! `tickets/` folder changed (TUR-166). The uncached [`highest_ticket_number`]
//! and [`super::highest_recorded_ticket_number`] are the rule it must agree
//! with.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use super::retired::highest_recorded_in;
use crate::stamp::Stamps;
use crate::{Error, MEETING_FILE, TICKETS_DIR, folder, ticket};

/// Each meeting folder's highest number, with the stamps of the two things it
/// was read from. Process-wide, like the numbering lock.
static PER_MEETING: LazyLock<Mutex<HashMap<PathBuf, (Stamps, u32)>>> =
    LazyLock::new(Default::default);

/// One past the highest ticket number anywhere under `root`: every ticket
/// file ([`highest_ticket_number`]) and every number a meeting's
/// `agent_tickets` record or `retired_tickets` list still names
/// ([`super::highest_recorded_ticket_number`]), so a number the user deleted
/// or discarded in one meeting is never handed out again in another.
///
/// A meeting whose `meeting.md` and `tickets/` look as they did last time is
/// not read again; the root's own `tickets/` is always listed.
///
/// A writer that then creates the ticket holds
/// [`super::lock_meeting_writers`] from this call until the file is on disk.
///
/// # Errors
///
/// [`Error::Io`] when `root` itself cannot be listed.
pub fn next_ticket_number(root: &Path) -> Result<u32, Error> {
    let dirs = folder::meeting_dirs(root)?;
    let mut cache = PER_MEETING.lock().unwrap_or_else(PoisonError::into_inner);
    let mut highest = highest_in(&root.join(TICKETS_DIR));
    for dir in &dirs {
        let (meeting_md, tickets) = (dir.join(MEETING_FILE), dir.join(TICKETS_DIR));
        let now = Stamps::of(&[&meeting_md, &tickets]).ok();
        let n = match (&now, cache.get(dir)) {
            (Some(now), Some((known, n))) if known.still(now) => *n,
            _ => {
                let n = highest_in(&tickets).max(highest_recorded_in(&meeting_md));
                match now {
                    Some(now) => cache.insert(dir.clone(), (now, n)),
                    None => cache.remove(dir),
                };
                n
            }
        };
        highest = highest.max(n);
    }
    // Forget folders that are gone, under this root only: another root's
    // entries stay for when it is the root again.
    cache.retain(|dir, _| dir.parent() != Some(root) || dirs.contains(dir));
    Ok(highest.saturating_add(1))
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
    Ok(folder::meeting_dirs(root)?
        .iter()
        .map(|dir| highest_in(&dir.join(TICKETS_DIR)))
        .fold(highest_in(&root.join(TICKETS_DIR)), u32::max))
}

/// The highest number among the ticket file names in one `tickets/` folder;
/// 0 for none, or when it is missing or cannot be listed (logged).
fn highest_in(dir: &Path) -> u32 {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return 0,
        Err(error) => {
            tracing::warn!(path = %dir.display(), %error, "skipping a tickets folder that could not be listed");
            return 0;
        }
    };
    entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".md")
                .and_then(ticket::parse_id)
        })
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_notes::highest_recorded_ticket_number;

    fn meeting(root: &Path, id: &str, extra: &str) -> std::path::PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).expect("meeting dir");
        let text = format!("---\nid: {id}\ntitle: Sync\n{extra}---\n\n## Notes\n");
        std::fs::write(dir.join(MEETING_FILE), text).expect("meeting.md");
        dir
    }

    #[test]
    fn an_empty_root_starts_at_one() {
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(next_ticket_number(root.path()).expect("next"), 1);
    }

    #[test]
    fn counts_files_in_every_meeting_and_the_shared_folder() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = meeting(root.path(), "2026-10-01-1000-a", "");
        std::fs::create_dir_all(a.join(TICKETS_DIR)).expect("tickets");
        std::fs::write(a.join(TICKETS_DIR).join("TICK-0007.md"), "x").expect("ticket");
        std::fs::create_dir_all(root.path().join(TICKETS_DIR)).expect("shared");
        std::fs::write(root.path().join(TICKETS_DIR).join("TICK-0003.md"), "x").expect("ticket");
        meeting(root.path(), "2026-10-02-1000-b", "");
        assert_eq!(next_ticket_number(root.path()).expect("next"), 8);
    }

    #[test]
    fn a_number_another_meeting_retired_is_never_handed_out() {
        let root = tempfile::tempdir().expect("tempdir");
        // Meeting A's record still names TICK-0010, whose file the user deleted.
        meeting(
            root.path(),
            "2026-10-01-1000-a",
            "agent_tickets:\n  TICK-0010: abc\n",
        );
        // Meeting C discarded TICK-0012.
        meeting(
            root.path(),
            "2026-10-03-1000-c",
            "retired_tickets:\n  - TICK-0012\n",
        );
        meeting(root.path(), "2026-10-02-1000-b", "");
        assert_eq!(next_ticket_number(root.path()).expect("next"), 13);
    }

    /// A file only: Windows opens no folder for writing its time.
    fn age(path: &Path) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| {
                file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            })
            .expect("set mtime");
    }

    fn uncached(root: &Path) -> u32 {
        highest_ticket_number(root)
            .expect("files")
            .max(highest_recorded_ticket_number(root).expect("records"))
            + 1
    }

    /// TUR-166: a meeting that did not change is not read again. Its
    /// `meeting.md` is swapped for one of the same size and time that names a
    /// higher number, and the cached answer still stands; touching it, or
    /// adding a ticket, is read.
    #[test]
    fn an_unchanged_meeting_is_not_read_again() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = meeting(
            root.path(),
            "2026-10-01-1000-a",
            "retired_tickets:\n  - TICK-0004\n",
        );
        // No `tickets/` yet: a missing folder needs no time to settle.
        let tickets = a.join(TICKETS_DIR);
        age(&a.join(MEETING_FILE));
        assert_eq!(next_ticket_number(root.path()).expect("next"), 5);

        let meeting_md = a.join(MEETING_FILE);
        let modified = std::fs::metadata(&meeting_md)
            .and_then(|meta| meta.modified())
            .expect("mtime");
        let text = std::fs::read_to_string(&meeting_md).expect("read");
        std::fs::write(&meeting_md, text.replace("TICK-0004", "TICK-0009")).expect("swap");
        std::fs::File::options()
            .write(true)
            .open(&meeting_md)
            .and_then(|file| file.set_modified(modified))
            .expect("same mtime");
        assert_eq!(next_ticket_number(root.path()).expect("cached"), 5);

        age(&meeting_md);
        assert_eq!(next_ticket_number(root.path()).expect("re-read"), 10);
        std::fs::create_dir_all(&tickets).expect("tickets");
        std::fs::write(tickets.join("TICK-0011.md"), "x").expect("ticket");
        assert_eq!(next_ticket_number(root.path()).expect("new ticket"), 12);
        assert_eq!(
            next_ticket_number(root.path()).expect("next"),
            uncached(root.path())
        );
    }

    #[test]
    fn the_cached_answer_agrees_with_a_full_read_as_meetings_come_and_go() {
        let root = tempfile::tempdir().expect("tempdir");
        let a = meeting(
            root.path(),
            "2026-10-01-1000-a",
            "agent_tickets:\n  TICK-0003: abc\n",
        );
        assert_eq!(
            next_ticket_number(root.path()).expect("next"),
            uncached(root.path())
        );
        let b = meeting(
            root.path(),
            "2026-10-02-1000-b",
            "retired_tickets:\n  - TICK-0006\n",
        );
        assert_eq!(next_ticket_number(root.path()).expect("next"), 7);
        std::fs::remove_dir_all(&b).expect("remove b");
        assert_eq!(next_ticket_number(root.path()).expect("next"), 4);
        assert_eq!(
            next_ticket_number(root.path()).expect("next"),
            uncached(root.path())
        );
        std::fs::remove_dir_all(&a).expect("remove a");
        assert_eq!(next_ticket_number(root.path()).expect("next"), 1);
    }
}
