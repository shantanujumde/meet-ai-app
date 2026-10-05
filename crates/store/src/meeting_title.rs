//! Who names a meeting, and which name wins (TUR-103).
//!
//! A meeting's title is `meeting.md`'s `title`, and `title_source` says who
//! wrote it ([`meeting_format::meeting_md::TITLE_SOURCE`]). Three writers, in
//! the order they usually come:
//!
//! 1. **The calendar** ([`crate::meeting_event::apply`]) names the meeting
//!    while it records, but only while it still has no title of its own
//!    ([`is_untitled`]). `title_source: calendar`.
//! 2. **The agent** suggests a title with its notes
//!    ([`crate::agent_notes::write`]). It replaces a calendar title, even a
//!    vague one like "Sync", and its own earlier one when notes run again.
//!    `title_source: agent`.
//! 3. **The user** renames the meeting from its page ([`set_by_user`]).
//!    `title_source: user`, and nothing replaces it after that.
//!
//! A title with no `title_source` that is not the folder-name default was
//! typed into `meeting.md` by hand, and is kept the same way a user's rename
//! is. So is a `title_source` this code does not know.
//!
//! Every title is cleaned the same way before it is written ([`clean`]): one
//! line, spaces collapsed, at most [`MAX_TITLE_CHARS`] characters. A title
//! that is blank after that is not written at all.

use std::io;
use std::path::Path;

use meeting_format::meeting_md::{TITLE, TITLE_SOURCE};

use crate::agent_notes::{default_title, lock_meeting_writers};
use crate::folder::meeting_dir;
use crate::meeting::Meeting;
use crate::{Error, MEETING_FILE};

/// The longest title written, in characters. Long enough for any real meeting
/// name; short enough that a model which answers with a sentence still leaves
/// a readable row in the meeting list.
pub const MAX_TITLE_CHARS: usize = 80;

/// Who wrote a meeting's title. Written as `title_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    Calendar,
    Agent,
    User,
}

impl TitleSource {
    /// The literal written to `title_source`.
    pub fn as_str(self) -> &'static str {
        match self {
            TitleSource::Calendar => "calendar",
            TitleSource::Agent => "agent",
            TitleSource::User => "user",
        }
    }

    /// The source `meeting` records, if it records one this code knows.
    pub fn of(meeting: &Meeting) -> Option<TitleSource> {
        match meeting.frontmatter.get_str(TITLE_SOURCE)?.as_str() {
            "calendar" => Some(TitleSource::Calendar),
            "agent" => Some(TitleSource::Agent),
            "user" => Some(TitleSource::User),
            _ => None,
        }
    }
}

/// `raw` as one line with single spaces, cut to [`MAX_TITLE_CHARS`], or
/// `None` when nothing is left.
pub fn clean(raw: &str) -> Option<String> {
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one_line.chars().take(MAX_TITLE_CHARS).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_owned())
}

/// The title is missing, blank, or still the folder-name default.
pub(crate) fn is_untitled(meeting: &Meeting, default: &str) -> bool {
    meeting
        .title()
        .is_none_or(|title| title.trim().is_empty() || title == default)
}

/// Write `title` (already [`clean`]ed) and `source` into `meeting`'s
/// frontmatter. Changes nothing on disk.
pub(crate) fn set(meeting: &mut Meeting, title: &str, source: TitleSource) {
    meeting.frontmatter.set_str(TITLE, Some(title));
    meeting
        .frontmatter
        .set_str(TITLE_SOURCE, Some(source.as_str()));
}

/// Give `meeting` the agent's suggested title, when the rules in the module
/// docs let it: the meeting is untitled, or its title came from the calendar
/// or from an earlier notes run. Returns whether the title was set.
///
/// `default` is the folder-name title ([`default_title`]).
pub(crate) fn suggest(meeting: &mut Meeting, suggested: &str, default: &str) -> bool {
    let Some(title) = clean(suggested) else {
        return false;
    };
    let replaceable = match TitleSource::of(meeting) {
        Some(TitleSource::User) => false,
        Some(TitleSource::Calendar | TitleSource::Agent) => true,
        None => is_untitled(meeting, default),
    };
    if replaceable {
        set(meeting, &title, TitleSource::Agent);
    }
    replaceable
}

/// Rename meeting `meeting_id` under `root` to `title`, as the user asked.
/// Returns the title as written ([`clean`]ed).
///
/// Creates `meeting.md` (id from the folder name, the four sections empty)
/// when the meeting has none yet. Holds [`lock_meeting_writers`] for the whole
/// read and write, like every other writer of `meeting.md`, so it cannot land
/// in the middle of a notes run.
///
/// The write is not recorded as this process's own ([`crate::watcher`]): the
/// watcher is meant to see it, so the search index learns the new name, as it
/// does for a calendar title.
///
/// # Errors
///
/// * [`Error::Io`] of kind `InvalidInput` for a title that is blank.
/// * [`Error::BadId`] for a meeting id that is not a plain folder name.
/// * [`Error::Io`] of kind `NotFound` when the meeting folder does not exist.
/// * [`Error::Frontmatter`] for a `meeting.md` with broken frontmatter, and
///   [`Error::Io`] of kind `InvalidData` for one that is not UTF-8. The file
///   is left as it was.
/// * [`Error::Io`] when the file cannot be read or written.
pub fn set_by_user(root: &Path, meeting_id: &str, title: &str) -> Result<String, Error> {
    let Some(title) = clean(title) else {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a meeting's name cannot be blank",
        )));
    };
    let _writers = lock_meeting_writers();

    let dir = meeting_dir(root, meeting_id)?;
    if !dir.is_dir() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::NotFound,
            format!("meeting folder {meeting_id} does not exist"),
        )));
    }
    let path = dir.join(MEETING_FILE);
    let mut meeting = match Meeting::read(&path)? {
        Some(meeting) => {
            // Refuse a file that cannot be written back before changing it.
            meeting.render()?;
            meeting
        }
        None => Meeting::new(meeting_id, &default_title(meeting_id)),
    };
    set(&mut meeting, &title, TitleSource::User);
    meeting.write(&path)?;
    Ok(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "2026-10-05-1000-meeting";

    fn meeting(title: &str, source: Option<&str>) -> Meeting {
        let mut meeting = Meeting::new(ID, title);
        if let Some(source) = source {
            meeting.frontmatter.set_str(TITLE_SOURCE, Some(source));
        }
        meeting
    }

    fn suggested(mut meeting: Meeting, title: &str) -> (bool, Meeting) {
        let set = suggest(&mut meeting, title, &default_title(ID));
        (set, meeting)
    }

    fn source(meeting: &Meeting) -> Option<String> {
        meeting.frontmatter.get_str(TITLE_SOURCE)
    }

    #[test]
    fn an_untitled_meeting_takes_the_agents_title() {
        let (set, meeting) = suggested(meeting(&default_title(ID), None), "Search launch plan");
        assert!(set);
        assert_eq!(meeting.title().as_deref(), Some("Search launch plan"));
        assert_eq!(source(&meeting).as_deref(), Some("agent"));
    }

    #[test]
    fn a_calendar_or_earlier_agent_title_is_replaced() {
        for from in ["calendar", "agent"] {
            let (set, meeting) = suggested(meeting("Sync", Some(from)), "Search launch plan");
            assert!(set, "{from}");
            assert_eq!(meeting.title().as_deref(), Some("Search launch plan"));
            assert_eq!(source(&meeting).as_deref(), Some("agent"));
        }
    }

    #[test]
    fn a_users_or_hand_typed_title_is_kept() {
        for (from, why) in [
            (Some("user"), "renamed"),
            (None, "typed"),
            (Some("x"), "unknown"),
        ] {
            let (set, meeting) = suggested(meeting("Budget review", from), "Search launch plan");
            assert!(!set, "{why}");
            assert_eq!(meeting.title().as_deref(), Some("Budget review"), "{why}");
            assert_eq!(source(&meeting).as_deref(), from, "{why}");
        }
    }

    #[test]
    fn a_blank_suggestion_changes_nothing() {
        for blank in ["", "   ", "\n\t"] {
            let (set, meeting) = suggested(meeting("Sync", Some("calendar")), blank);
            assert!(!set);
            assert_eq!(meeting.title().as_deref(), Some("Sync"));
            assert_eq!(source(&meeting).as_deref(), Some("calendar"));
        }
    }

    #[test]
    fn titles_are_one_trimmed_line_and_capped() {
        assert_eq!(
            clean("  Search\n launch   plan ").as_deref(),
            Some("Search launch plan")
        );
        let long = "word ".repeat(40);
        let cut = clean(&long).unwrap();
        assert!(cut.chars().count() <= MAX_TITLE_CHARS);
        assert!(!cut.ends_with(' '));
        // Cut on a character, not a byte.
        assert_eq!(
            clean(&"é".repeat(100)).unwrap().chars().count(),
            MAX_TITLE_CHARS
        );
        assert_eq!(clean(" \n "), None);
    }

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

    #[test]
    fn a_user_rename_is_written_with_its_source() {
        let root = root_with_meeting();
        let path = root.path().join(ID).join(MEETING_FILE);
        let mut before = meeting("Sync", Some("agent"));
        before.frontmatter.set_str("repo", Some("~/apps/api"));
        before.write(&path).unwrap();

        let written = set_by_user(root.path(), ID, "  Budget\nreview ").unwrap();
        assert_eq!(written, "Budget review");
        let after = read(root.path());
        assert_eq!(after.title().as_deref(), Some("Budget review"));
        assert_eq!(source(&after).as_deref(), Some("user"));
        assert_eq!(
            after.frontmatter.get_str("repo").as_deref(),
            Some("~/apps/api")
        );

        // ...and the agent leaves it alone from then on.
        let (set, kept) = suggested(after, "Search launch plan");
        assert!(!set);
        assert_eq!(kept.title().as_deref(), Some("Budget review"));
    }

    #[test]
    fn a_user_rename_creates_meeting_md_when_there_is_none() {
        let root = root_with_meeting();
        set_by_user(root.path(), ID, "Budget review").unwrap();
        let meeting = read(root.path());
        assert_eq!(meeting.id().as_deref(), Some(ID));
        assert_eq!(meeting.title().as_deref(), Some("Budget review"));
    }

    #[test]
    fn a_blank_rename_is_refused_and_nothing_is_written() {
        let root = root_with_meeting();
        let error = set_by_user(root.path(), ID, "  \n").unwrap_err();
        assert!(matches!(error, Error::Io(ref io) if io.kind() == io::ErrorKind::InvalidInput));
        assert!(!root.path().join(ID).join(MEETING_FILE).exists());
    }

    #[test]
    fn a_rename_refuses_broken_frontmatter_a_missing_folder_and_a_bad_id() {
        let root = root_with_meeting();
        let path = root.path().join(ID).join(MEETING_FILE);
        let broken = "---\ntitle: [unclosed\n---\n\n## Summary\n";
        std::fs::write(&path, broken).unwrap();
        assert!(set_by_user(root.path(), ID, "Budget review").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);

        let empty = tempfile::tempdir().unwrap();
        assert!(set_by_user(empty.path(), ID, "Budget review").is_err());
        assert!(set_by_user(empty.path(), "../escape", "Budget review").is_err());
    }
}
