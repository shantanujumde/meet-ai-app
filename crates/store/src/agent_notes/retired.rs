//! The numbers [`super::write()`] keeps retired, for the other writers of new
//! ticket numbers (TUR-18).

use std::path::Path;

use yaml_rust2::Yaml;

use super::AGENT_TICKETS_KEY;
use crate::meeting::Meeting;
use crate::{Error, MEETING_FILE, folder, ticket};

/// The highest ticket number any meeting's `agent_tickets` record names,
/// tickets the user deleted included.
///
/// [`super::write()`] never hands out a number its own record still names, so
/// a deleted top ticket does not come back under its old name. A writer that
/// numbers tickets for the whole root, such as a hand-made ticket, takes the
/// larger of this and [`super::highest_ticket_number`], under
/// [`super::lock_ticket_numbers`], to keep the same rule.
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
