//! The one rule every writer of a new ticket number follows (TUR-154): a
//! notes run, a hand-made ticket and the copied wrap-up prompt all number
//! tickets for the whole meetings root, never per meeting, and never reuse a
//! number any meeting still records or retired.

use std::path::Path;

use super::{highest_recorded_ticket_number, highest_ticket_number};
use crate::Error;

/// One past the highest ticket number anywhere under `root`: every ticket
/// file ([`highest_ticket_number`]) and every number a meeting's
/// `agent_tickets` record or `retired_tickets` list still names
/// ([`highest_recorded_ticket_number`]), so a number the user deleted or
/// discarded in one meeting is never handed out again in another.
///
/// A writer that then creates the ticket holds
/// [`super::lock_meeting_writers`] from this call until the file is on disk.
///
/// # Errors
///
/// [`Error::Io`] when `root` itself cannot be listed.
pub fn next_ticket_number(root: &Path) -> Result<u32, Error> {
    Ok(highest_ticket_number(root)?
        .max(highest_recorded_ticket_number(root)?)
        .saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MEETING_FILE, TICKETS_DIR};

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
}
