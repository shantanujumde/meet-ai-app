//! The per-meeting "Make notes for this meeting" switch (SPEC A11, "Skip one
//! meeting", TUR-12).
//!
//! Off is written into the meeting's own `meeting.md` as `agent_notes: off`,
//! so it lives with the meeting: it survives a rescan, a deleted `index.db`
//! and a move of the meetings folder (L7). The notes run reads it before it
//! sends anything, and [`crate::agent_notes::write`] reads it again before it
//! writes anything, so a run already going when the switch goes off writes
//! nothing.
//!
//! On removes the key again, which is the default for every meeting. Nothing
//! else in the file changes: every other frontmatter key and every section
//! stays as it was, because the file goes through [`Meeting`]. A `meeting.md`
//! whose frontmatter is broken, or that is not UTF-8, is refused rather than
//! overwritten.

use std::io;
use std::path::Path;

use crate::agent_notes::{AGENT_NOTES_KEY, default_title, lock_meeting_md, notes_are_off};
use crate::folder::meeting_dir;
use crate::meeting::Meeting;
use crate::watcher::SelfWrites;
use crate::{Error, MEETING_FILE};

/// The value written when the switch is off. The YAML writer quotes it
/// (`agent_notes: "off"`) so an older YAML reader does not take it for
/// `false`; read back, both spellings mean off.
pub(crate) const OFF: &str = "off";

/// Turn notes for meeting `meeting_id` under `root` on or off.
///
/// Off writes `agent_notes: off`, creating `meeting.md` (id and title from
/// the folder name, the four sections empty) when the meeting has none yet.
/// On removes the key; with no `meeting.md` there is nothing to remove, so
/// nothing is written. When the file already says what was asked, it is not
/// touched.
///
/// Holds [`lock_meeting_md`] for the whole read and write, so it cannot
/// land in the middle of a notes run writing the same file.
///
/// # Errors
///
/// * [`Error::BadId`] for a meeting id that is not a plain folder name.
/// * [`Error::Io`] of kind `NotFound` when the meeting folder does not exist.
/// * [`Error::Frontmatter`] for a `meeting.md` with broken frontmatter, and
///   [`Error::Io`] of kind `InvalidData` for one that is not UTF-8. The file
///   is left as it was.
/// * [`Error::Io`] when the file cannot be read or written.
pub fn set(root: &Path, meeting_id: &str, on: bool, self_writes: &SelfWrites) -> Result<(), Error> {
    let _meeting_md = lock_meeting_md();

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
            // Refuse a file that cannot be written back, whichever way the
            // switch goes: with its frontmatter unreadable there is no telling
            // what it says.
            meeting.render()?;
            meeting
        }
        None if on => return Ok(()),
        None => Meeting::new(meeting_id, &default_title(meeting_id)),
    };

    if on {
        if meeting.frontmatter.remove(AGENT_NOTES_KEY).is_none() {
            return Ok(());
        }
    } else {
        if is_off(&meeting) {
            return Ok(());
        }
        meeting.frontmatter.set_str(AGENT_NOTES_KEY, Some(OFF));
    }
    meeting.write(&path)?;
    self_writes.note(&path);
    Ok(())
}

/// The meeting is marked `agent_notes: off` (or `false`): no notes run sends
/// it, and none writes into it.
pub fn is_off(meeting: &Meeting) -> bool {
    notes_are_off(meeting)
}
