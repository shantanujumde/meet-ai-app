//! On-disk storage for meet-ai: reading and writing the meetings folder.
//!
//! Phase 3a (SPEC §5, TUR-99). Every file in SPEC §3.1 is read here, and every
//! one but the transcript is written here too:
//!
//! * [`meeting`] — `meeting.md` (§3.2): frontmatter plus the four fixed
//!   sections.
//! * [`transcript`] — `transcript.md` (§3.4): read only. The strict line
//!   format is appended by `stt`'s sink and nothing else.
//! * [`transcript_order`] — the one rewrite of `transcript.md`: put in time
//!   order once, when a recording stops (SPEC A19).
//! * [`notes`] — `notes.md`: the user's own notes.
//! * [`ticket`] — `tickets/TICK-NNNN.md` (§3.3).
//! * [`agent_notes`] — `meeting.md` sections and tickets written from the
//!   agent's notes JSON (SPEC A11).
//! * [`notes_switch`] — the per-meeting `agent_notes: off` switch (SPEC A11).
//! * [`meeting_event`] — `meeting.md`'s title, attendees and event id, from
//!   the calendar event the meeting was recorded during (TUR-29).
//! * [`meeting_title`] — whose title wins: the calendar's, the agent's or the
//!   user's (TUR-103).
//! * [`folder`] — one meeting folder, or every folder under the root, loaded
//!   together.
//! * [`retention`] — deleting meeting audio after `retention_days` (L16).
//!
//! Four rules from the spec are worth reading before adding anything here,
//! because all four are easy to violate by accident:
//!
//! * **L7 — markdown is the truth.** `index.db` is derived and must be safe to
//!   delete. The Phase 3 exit gate is literally "delete `index.db`, everything
//!   still works after a rescan".
//! * **Unknown frontmatter keys survive a round trip.** An agent, a future
//!   version or the user may put keys there this code has never heard of.
//!   Reading a file and writing it back must not drop them — see
//!   [`frontmatter`].
//! * **A malformed file must not take the app down (SPEC §7).** It loads, it
//!   carries a [`Problem`] so the UI can show a "needs attention" badge, and
//!   the files either side of it still load. Only an unreadable *folder* is an
//!   [`Error`].
//! * **SPEC §4 — the watcher must suppress this process's own writes.** The app
//!   writes `notes.md` while the user types in it. Without suppression the
//!   watcher fires, the app reloads, and the cursor jumps mid-sentence. Files an
//!   agent writes itself (the copy-prompt path) are *not* suppressed; the notes
//!   the app writes from the agent's JSON ([`agent_notes`]) are. (Phase 3b,
//!   TUR-100; TUR-7.)
//!
//! `store` must stay free of mac-only code (SPEC §8.2): every path is built
//! with `Path::join`, and the only OS `cfg` in the crate is in `platform/`
//! (quality rule R10), which retention asks which error codes mean a file is
//! held open.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::path::Path;
use std::time::Duration;

pub mod agent_notes;
pub mod folder;
pub mod folder_name;
pub mod frontmatter;
pub mod index;
pub mod meeting;
pub mod meeting_event;
pub mod meeting_title;
pub mod notes;
pub mod notes_switch;
mod platform;

/// Create `<root>/.app` (and its parents) and hide it where a dot name does
/// not (Windows). Every writer that may be first to make `.app` calls this,
/// so a fresh install's folder is hidden without waiting for a restart.
/// Hiding is best effort: a failure is logged, never returned.
pub fn create_app_dir(app_dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(app_dir)?;
    platform::hide_app_dir(app_dir);
    Ok(())
}
pub mod retention;
pub mod ticket;
pub mod transcript;
pub mod transcript_order;
pub mod watcher;

/// The SPEC §3.1 folder layout, defined once in `meeting-format`.
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::layout;
/// File names inside a meeting folder (SPEC §3.1), re-exported at their old
/// paths.
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::layout::{MEETING_FILE, NOTES_FILE, TRANSCRIPT_FILE};
/// Tickets are store's alone (§3.3), so their folder name stays here.
pub const TICKETS_DIR: &str = "tickets";

/// How long a path this process wrote stays ignored by the watcher.
///
/// SPEC §4. Longer than the `notify` debounce, short enough that a real external
/// edit landing right after ours is not swallowed.
pub const SELF_WRITE_SUPPRESSION: Duration = Duration::from_millis(750);

/// How long `notify` events are coalesced before the watcher acts on them.
///
/// Agent writes arrive as a burst of files; debouncing turns that into one
/// reload instead of a dozen.
pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(500);

/// Everything that can go wrong reading or writing the meetings folder.
///
/// A file that is merely *malformed* is not an error — it loads with a
/// [`Problem`] attached. These are the cases where there is nothing to load or
/// writing would destroy data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file under the meetings root could not be read or written.
    #[error("could not read or write the meetings folder")]
    Io(#[from] std::io::Error),

    /// A frontmatter block was not valid YAML.
    ///
    /// Raised on *write* only: a file that loaded with broken frontmatter is
    /// refused rather than overwritten, because writing it back would replace
    /// whatever the user or agent put there with an empty block.
    #[error("the frontmatter in {path} is not valid YAML: {detail}")]
    Frontmatter { path: String, detail: String },

    /// A meeting or ticket id that is not a plain file or folder name.
    ///
    /// Ids arrive from the webview, so `..` or an absolute path would otherwise
    /// let a compromised page read and write anywhere the app can reach.
    #[error("{0:?} is not a valid id")]
    BadId(String),

    /// The derived index could not be opened, migrated or queried.
    ///
    /// Always recoverable: delete `index.db` and rescan.
    #[error("the search index is unusable and needs rebuilding")]
    Index(String),
}

/// Something wrong with a file that did not stop it loading.
///
/// SPEC §7: the UI shows a "needs attention" badge instead of failing. An agent
/// writing sloppy markdown is an expected condition, not a bug report, so every
/// variant here describes the file, not a fault in this crate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Problem {
    /// The file could not be read as UTF-8 text.
    Unreadable { detail: String },
    /// No `---` frontmatter block at the top of a file that needs one.
    NoFrontmatter,
    /// The frontmatter block is there but is not valid YAML, or is not a
    /// mapping of keys to values.
    BadFrontmatter { detail: String },
    /// A key the spec requires is missing (e.g. a ticket with no `id`).
    MissingField { key: String },
    /// A known key has a value of the wrong shape (e.g. `status: maybe`).
    BadField { key: String, detail: String },
    /// A fixed §3.2 heading (`## Summary` etc.) is not in `meeting.md`.
    MissingSection { heading: String },
    /// `transcript.md` lines that did not match §3.4 and were skipped.
    UnparsedLines { count: usize },
}

/// The error a render must return instead of overwriting what it could not
/// load, if any. Shared by meetings and tickets: the rule is the same for both
/// files.
pub(crate) fn refusal(problems: &[Problem], path: &str) -> Option<Error> {
    problems.iter().find_map(|problem| match problem {
        Problem::BadFrontmatter { detail } => Some(Error::Frontmatter {
            path: path.to_owned(),
            detail: detail.clone(),
        }),
        Problem::Unreadable { detail } => Some(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{path} was not readable as UTF-8 ({detail}); refusing to overwrite it"),
        ))),
        _ => None,
    })
}

/// Write `contents` to `path` through a sibling temp file and `rename(2)`, so a
/// crash mid-write can never leave a half-written file behind.
///
/// Creates the folder first, then defers to [`meeting_format::write_atomic`]:
/// the temp file is a dotfile in the same folder — same filesystem, so the
/// rename is atomic, and the folder scan already skips dotfiles — and both the
/// file and the folder are `fsync`'d, so a saved note survives a power cut and
/// not just a crash. A failed write removes its temp file.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), Error> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    Ok(meeting_format::write_atomic(path, contents.as_bytes())?)
}

/// Is `id` a single plain path component — no separators, no `..`, no leading
/// dot? Shared by meeting and ticket ids.
///
/// `:` and NUL are refused on every OS, not only where they bite: on Windows
/// `C:` is a drive prefix (so `root.join("C:")` leaves the root) and `a:b`
/// names an alternate data stream; NUL is never valid in a path. No meeting
/// folder name (`YYYY-MM-DD-HHMM-slug`) or ticket id contains either.
pub(crate) fn is_plain_name(id: &str) -> bool {
    let mut components = Path::new(id).components();
    !id.is_empty()
        && !id.starts_with('.')
        && !id.contains(['/', '\\', ':', '\0'])
        && matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_is_available() {
        // L7 makes index.db a full-text search index, and rusqlite 0.40 has no
        // `fts5` feature flag to ask for it. This proves the bundled
        // amalgamation was compiled with FTS5 on, so Phase 3 does not discover
        // otherwise.
        let db = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        db.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body);")
            .expect("FTS5 is missing from the bundled SQLite build");
    }

    #[test]
    fn suppression_outlives_the_debounce_window() {
        // If this ever inverts, a self-write echo arrives after suppression has
        // already expired and the user's cursor jumps. Guard it with a test
        // rather than a comment.
        assert!(SELF_WRITE_SUPPRESSION > WATCH_DEBOUNCE);
    }

    #[test]
    fn an_id_cannot_escape_its_folder() {
        for hostile in ["..", "../../etc", "/etc/passwd", ".app", "", "a/b", "a\\b"] {
            assert!(!is_plain_name(hostile), "{hostile:?} must be refused");
        }
        assert!(is_plain_name("2026-09-01-1430-standup"));
        assert!(is_plain_name("TICK-0001"));
    }

    #[test]
    fn atomic_write_leaves_no_temp_file_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let path = dir.join("notes.md");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("notes.md")]);
    }
}
