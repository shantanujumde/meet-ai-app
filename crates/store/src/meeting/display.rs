//! The title and date a meeting is shown with when `meeting.md` does not say
//! (TUR-176): the one fallback the meeting list, a notes run, Copy prompt and
//! Sync share, read from the folder name (SPEC §3.1).

use super::Meeting;
use crate::folder_name::{prettify_slug, split_folder_name};

/// `meeting.md`'s title, or else [`folder_title`].
pub fn display_title(meeting: Option<&Meeting>, meeting_id: &str) -> String {
    meeting
        .and_then(Meeting::title)
        .unwrap_or_else(|| folder_title(meeting_id))
}

/// The folder's slug made readable, or the id itself for a folder someone
/// renamed by hand.
pub fn folder_title(meeting_id: &str) -> String {
    match split_folder_name(meeting_id) {
        (_, _, Some(slug)) => prettify_slug(slug),
        _ => meeting_id.to_owned(),
    }
}

/// `meeting.md`'s `date`, or else the day and time from the folder name,
/// trimmed. `None` when neither has one.
pub fn display_date(meeting: Option<&Meeting>, meeting_id: &str) -> Option<String> {
    let (day, time, _) = split_folder_name(meeting_id);
    meeting
        .and_then(Meeting::date)
        .or_else(|| day.map(|day| format!("{day} {}", time.unwrap_or_default())))
        .map(|date| date.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "2026-09-01-1430-platform-standup";

    #[test]
    fn with_no_meeting_md_the_folder_name_is_used() {
        assert_eq!(display_title(None, ID), "Platform standup");
        assert_eq!(display_date(None, ID).as_deref(), Some("2026-09-01 14:30"));
    }

    #[test]
    fn a_hand_renamed_folder_is_titled_by_its_name_and_has_no_date() {
        assert_eq!(display_title(None, "my-old-notes"), "my-old-notes");
        assert_eq!(folder_title("my-old-notes"), "my-old-notes");
        assert_eq!(display_date(None, "my-old-notes"), None);
    }

    #[test]
    fn meeting_md_wins_and_its_date_is_trimmed() {
        let raw = "---\ntitle: Weekly sync\ndate: \" 2026-09-01T14:30:00+05:30 \"\n---\n";
        let meeting = Meeting::parse(raw);
        assert_eq!(display_title(Some(&meeting), ID), "Weekly sync");
        assert_eq!(
            display_date(Some(&meeting), ID).as_deref(),
            Some("2026-09-01T14:30:00+05:30")
        );
    }
}
