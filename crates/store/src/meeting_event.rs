//! Naming a meeting from the calendar event it was recorded during (TUR-29).
//!
//! `docs/problem.md` item 40: the meeting names itself, so the user never types
//! a title. The app picks the event (`calendar::matching`) and hands its title,
//! attendees and id to [`apply`], which writes them into `meeting.md`'s
//! frontmatter (SPEC §3.2) under the keys in [`meeting_format::meeting_md`].
//!
//! What is already in the file wins, every time:
//!
//! * `title` is set only while the meeting still has its default title, the
//!   one [`Meeting::new`] gives a fresh file (the folder name). A title the
//!   user typed is never replaced. When it is set, `title_source: calendar`
//!   goes with it, so the agent's suggestion may replace it later
//!   ([`crate::meeting_title`]).
//! * `attendees` is set only when the file names nobody yet.
//! * `calendar_event_id` is set only when the file has none.
//!
//! The folder is never renamed: its id stays the start time the recorder gave
//! it, so nothing that already points at it breaks.
//!
//! The write is not recorded as this process's own ([`crate::watcher`]): the
//! watcher is meant to see it, so the meeting list and the search index pick
//! up the new title and attendees while the recording is still going.

use std::io;
use std::path::Path;

use meeting_format::meeting_md::{ATTENDEES, CALENDAR_EVENT_ID};
use yaml_rust2::Yaml;

use crate::agent_notes::{default_title, lock_ticket_numbers};
use crate::folder::meeting_dir;
use crate::meeting::Meeting;
use crate::meeting_title::{self, TitleSource, is_untitled};
use crate::{Error, MEETING_FILE};

/// What the calendar says about the meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FromCalendar<'a> {
    /// The event's id within its calendar.
    pub event_id: &'a str,
    /// The event's title.
    pub title: &'a str,
    /// Who is invited, by name (display name, else email address).
    pub attendees: &'a [String],
}

/// Which keys [`apply`] wrote. All `false` means the file already said
/// everything it could, and nothing was written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub title: bool,
    pub attendees: bool,
    pub calendar_event_id: bool,
}

impl Applied {
    /// Something was written.
    pub fn any(self) -> bool {
        self.title || self.attendees || self.calendar_event_id
    }
}

/// Write `event` into meeting `meeting_id` under `root`. See the module docs
/// for which keys are kept.
///
/// Creates `meeting.md` (id and default title from the folder name, the four
/// sections empty) when the meeting has none yet, which is the normal case
/// for a recording that has just started.
///
/// Holds [`lock_ticket_numbers`] for the whole read and write, so it cannot
/// land in the middle of a notes run or the notes switch writing the same
/// file.
///
/// # Errors
///
/// * [`Error::BadId`] for a meeting id that is not a plain folder name.
/// * [`Error::Io`] of kind `NotFound` when the meeting folder does not exist.
/// * [`Error::Frontmatter`] for a `meeting.md` with broken frontmatter, and
///   [`Error::Io`] of kind `InvalidData` for one that is not UTF-8. The file
///   is left as it was.
/// * [`Error::Io`] when the file cannot be read or written.
pub fn apply(root: &Path, meeting_id: &str, event: &FromCalendar<'_>) -> Result<Applied, Error> {
    let _writers = lock_ticket_numbers();

    let dir = meeting_dir(root, meeting_id)?;
    if !dir.is_dir() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("meeting folder {meeting_id} does not exist"),
        )));
    }
    let path = dir.join(MEETING_FILE);
    let default = default_title(meeting_id);
    let mut meeting = match Meeting::read(&path)? {
        Some(meeting) => {
            // Refuse a file that cannot be written back before changing it.
            meeting.render()?;
            meeting
        }
        None => Meeting::new(meeting_id, &default),
    };

    let mut applied = Applied::default();
    if let Some(title) = meeting_title::clean(event.title)
        && is_untitled(&meeting, &default)
    {
        meeting_title::set(&mut meeting, &title, TitleSource::Calendar);
        applied.title = true;
    }
    if !event.attendees.is_empty() && meeting.attendees().is_empty() {
        let names = event.attendees.iter().cloned().map(Yaml::String).collect();
        meeting.frontmatter.set(ATTENDEES, Yaml::Array(names));
        applied.attendees = true;
    }
    if !event.event_id.is_empty() && meeting.frontmatter.get_str(CALENDAR_EVENT_ID).is_none() {
        meeting
            .frontmatter
            .set_str(CALENDAR_EVENT_ID, Some(event.event_id));
        applied.calendar_event_id = true;
    }

    if applied.any() {
        meeting.write(&path)?;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use meeting_format::meeting_md::TITLE_SOURCE;

    use super::*;

    const ID: &str = "2026-10-05-1000-meeting";

    fn root_with_meeting() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(ID)).unwrap();
        root
    }

    fn read(root: &Path) -> Meeting {
        Meeting::read(&root.join(ID).join(MEETING_FILE))
            .unwrap()
            .unwrap()
    }

    fn standup(attendees: &[String]) -> FromCalendar<'_> {
        FromCalendar {
            event_id: "EVT-1",
            title: "Platform Standup",
            attendees,
        }
    }

    fn names() -> Vec<String> {
        vec!["Shantanu".into(), "priya@example.com".into()]
    }

    #[test]
    fn a_fresh_meeting_takes_the_title_attendees_and_event_id() {
        let root = root_with_meeting();
        let names = names();
        let applied = apply(root.path(), ID, &standup(&names)).unwrap();
        assert!(applied.title && applied.attendees && applied.calendar_event_id);

        let meeting = read(root.path());
        assert_eq!(meeting.id().as_deref(), Some(ID));
        assert_eq!(meeting.title().as_deref(), Some("Platform Standup"));
        assert_eq!(
            meeting.frontmatter.get_str(TITLE_SOURCE).as_deref(),
            Some("calendar")
        );
        assert_eq!(meeting.attendees(), names);
        assert_eq!(
            meeting.frontmatter.get_str(CALENDAR_EVENT_ID).as_deref(),
            Some("EVT-1")
        );
        assert!(meeting.problems.is_empty(), "{:?}", meeting.problems);
        // The folder keeps its name.
        assert!(root.path().join(ID).is_dir());
    }

    #[test]
    fn a_hand_typed_title_is_kept() {
        let root = root_with_meeting();
        let path = root.path().join(ID).join(MEETING_FILE);
        Meeting::new(ID, "Budget review").write(&path).unwrap();

        let names = names();
        let applied = apply(root.path(), ID, &standup(&names)).unwrap();
        assert!(!applied.title);
        assert!(applied.attendees && applied.calendar_event_id);
        let meeting = read(root.path());
        assert_eq!(meeting.title().as_deref(), Some("Budget review"));
        assert_eq!(meeting.frontmatter.get_str(TITLE_SOURCE), None);
    }

    #[test]
    fn the_default_title_counts_as_untitled() {
        let root = root_with_meeting();
        let path = root.path().join(ID).join(MEETING_FILE);
        Meeting::new(ID, &default_title(ID)).write(&path).unwrap();

        let applied = apply(root.path(), ID, &standup(&[])).unwrap();
        assert!(applied.title);
        assert!(!applied.attendees, "no names, so nothing to write");
        assert_eq!(
            read(root.path()).title().as_deref(),
            Some("Platform Standup")
        );
    }

    #[test]
    fn attendees_and_event_id_already_there_are_kept_and_nothing_is_rewritten() {
        let root = root_with_meeting();
        let path = root.path().join(ID).join(MEETING_FILE);
        let mut meeting = Meeting::new(ID, "Budget review");
        meeting
            .frontmatter
            .set(ATTENDEES, Yaml::Array(vec![Yaml::String("Dev".into())]));
        meeting.frontmatter.set_str(CALENDAR_EVENT_ID, Some("OLD"));
        meeting.write(&path).unwrap();
        let before = std::fs::read(&path).unwrap();

        let names = names();
        let applied = apply(root.path(), ID, &standup(&names)).unwrap();
        assert_eq!(applied, Applied::default());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_numeric_looking_event_id_stays_a_string() {
        let root = root_with_meeting();
        let event = FromCalendar {
            event_id: "123",
            ..standup(&[])
        };
        apply(root.path(), ID, &event).unwrap();
        let meeting = read(root.path());
        assert_eq!(
            meeting.frontmatter.get(CALENDAR_EVENT_ID),
            Some(&Yaml::String("123".into()))
        );
    }

    #[test]
    fn broken_frontmatter_is_refused_and_left_alone() {
        let root = root_with_meeting();
        let path = root.path().join(ID).join(MEETING_FILE);
        let broken = "---\ntitle: [unclosed\n---\n\n## Summary\n";
        std::fs::write(&path, broken).unwrap();
        assert!(apply(root.path(), ID, &standup(&[])).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn a_missing_folder_or_a_bad_id_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        assert!(apply(root.path(), ID, &standup(&[])).is_err());
        assert!(apply(root.path(), "../escape", &standup(&[])).is_err());
    }
}
