//! Meetings by title, from the `meetings` table (TUR-32).
//!
//! The pre-meeting brief needs "the last meeting with this name". That is one
//! read of the derived index, not a walk over every folder on disk.

use super::{Index, index_error};
use crate::Error;

/// One row of the index's `meetings` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedMeeting {
    /// The folder name, e.g. `2026-09-01-1430-standup`.
    pub id: String,
    /// The `title` key, or the folder name when the meeting has none.
    pub title: String,
    /// The `date` key as written, e.g. `2026-09-01T14:30:00+05:30`.
    pub date: Option<String>,
}

impl Index {
    /// Every meeting whose title is `title`, ignoring case and surrounding
    /// whitespace. Newest folder name first. A blank title matches nothing.
    pub fn meetings_titled(&self, title: &str) -> Result<Vec<IndexedMeeting>, Error> {
        let wanted = same_title_key(title);
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        // SQLite's `lower()` only folds ASCII, so the comparison is made here,
        // where `to_lowercase` handles every script. The table has one row per
        // meeting, so this stays small.
        let mut statement = self
            .conn
            .prepare("SELECT id, title, date FROM meetings ORDER BY id DESC")
            .map_err(index_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(IndexedMeeting {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    date: row.get(2)?,
                })
            })
            .map_err(index_error)?;
        let mut found = Vec::new();
        for row in rows {
            let meeting = row.map_err(index_error)?;
            if same_title_key(&meeting.title) == wanted {
                found.push(meeting);
            }
        }
        Ok(found)
    }
}

/// What two titles must share to count as the same meeting: trimmed, any case.
pub fn same_title_key(title: &str) -> String {
    title.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_match_ignoring_case_and_outer_spaces() {
        assert_eq!(
            same_title_key("  Platform Standup "),
            same_title_key("platform standup")
        );
        assert_eq!(same_title_key("ÉQUIPE"), same_title_key("équipe"));
        assert_ne!(same_title_key("Standup"), same_title_key("Stand up"));
    }
}
