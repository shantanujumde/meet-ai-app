//! The numbers [`super::write()`] keeps retired, for the other writers of new
//! ticket numbers (TUR-18), and the numbers a discarded suggestion retires
//! (TUR-113).

use std::path::Path;

use yaml_rust2::Yaml;

use super::{AGENT_TICKETS_KEY, actions};
use crate::meeting::Meeting;
use crate::ticket::Ticket;
use crate::{Error, MEETING_FILE, TICKETS_DIR, folder, ticket};

/// The `meeting.md` frontmatter key listing the ticket ids the user discarded
/// from this meeting's suggested tasks (SPEC A26). A list of ids; no writer
/// hands any of them out again.
pub const RETIRED_TICKETS_KEY: &str = "retired_tickets";

/// The `meeting.md` frontmatter key listing the titles of the suggested tasks
/// the user discarded, so a notes re-run does not suggest them again (SPEC
/// A26). Compared without case or outer spaces.
pub const RETIRED_TITLES_KEY: &str = "retired_titles";

/// The highest ticket number any meeting's `agent_tickets` record or
/// `retired_tickets` list names, tickets the user deleted or discarded
/// included.
///
/// [`super::write()`] never hands out a number any meeting's record still
/// names, so a deleted top ticket does not come back under its old name, in
/// its own meeting or another. Every writer of a new number goes through
/// [`super::next_ticket_number`], under [`super::lock_meeting_writers`], to
/// keep the same rule.
///
/// A `meeting.md` that cannot be read is logged and skipped, as an unlistable
/// `tickets/` is in [`super::highest_ticket_number`].
///
/// # Errors
///
/// [`Error::Io`] when `root` itself cannot be listed.
pub fn highest_recorded_ticket_number(root: &Path) -> Result<u32, Error> {
    let mut highest = 0;
    for dir in folder::meeting_dirs(root)? {
        let path = dir.join(MEETING_FILE);
        let meeting = match Meeting::read(&path) {
            Ok(Some(meeting)) => meeting,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skipping a meeting.md that could not be read");
                continue;
            }
        };
        highest = highest.max(highest_retired(&meeting));
        let Some(Yaml::Hash(record)) = meeting.frontmatter.get(AGENT_TICKETS_KEY) else {
            continue;
        };
        for id in record.keys().filter_map(Yaml::as_str) {
            if let Some(n) = ticket::parse_id(id) {
                highest = highest.max(n);
            }
        }
    }
    Ok(highest)
}

/// The highest number in `meeting`'s `retired_tickets` list; 0 for none.
fn highest_retired(meeting: &Meeting) -> u32 {
    meeting
        .frontmatter
        .get_str_list(RETIRED_TICKETS_KEY)
        .unwrap_or_default()
        .iter()
        .filter_map(|id| ticket::parse_id(id))
        .max()
        .unwrap_or(0)
}

/// Add `ticket_id` to `meeting`'s `retired_tickets` list, once.
pub(crate) fn retire(meeting: &mut Meeting, ticket_id: &str) {
    let mut ids = meeting
        .frontmatter
        .get_str_list(RETIRED_TICKETS_KEY)
        .unwrap_or_default();
    if ids.iter().any(|id| id == ticket_id) {
        return;
    }
    ids.push(ticket_id.to_owned());
    let list = ids.into_iter().map(Yaml::String).collect();
    meeting
        .frontmatter
        .set(RETIRED_TICKETS_KEY, Yaml::Array(list));
}

/// Add `title` to `meeting`'s `retired_titles` list, once.
pub(crate) fn retire_title(meeting: &mut Meeting, title: &str) {
    let mut titles = meeting
        .frontmatter
        .get_str_list(RETIRED_TITLES_KEY)
        .unwrap_or_default();
    if titles.iter().any(|t| same_title(t, title)) {
        return;
    }
    titles.push(title.trim().to_owned());
    let list = titles.into_iter().map(Yaml::String).collect();
    meeting
        .frontmatter
        .set(RETIRED_TITLES_KEY, Yaml::Array(list));
}

/// Whether two ticket titles are the same task.
pub(super) fn same_title(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The titles a notes run must not suggest again for `meeting_id`: those of
/// its approved tickets (root `tickets/` files naming it) and the ones in its
/// `retired_titles` list.
pub(super) fn settled_titles(root: &Path, meeting_id: &str, meeting: &Meeting) -> Vec<String> {
    let mut titles = meeting
        .frontmatter
        .get_str_list(RETIRED_TITLES_KEY)
        .unwrap_or_default();
    for (_, _, path) in actions::ticket_files(&root.join(TICKETS_DIR)) {
        let Ok(found) = Ticket::read(&path) else {
            continue;
        };
        if found.meeting().is_some_and(|m| m == meeting_id) {
            titles.extend(found.title());
        }
    }
    titles
}
