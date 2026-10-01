//! One meeting folder, or every folder under the meetings root, loaded
//! together.
//!
//! This is where SPEC §7's rule is enforced end to end: one broken file never
//! stops the others in its folder loading, and one broken folder never stops
//! the rest of the list.
//!
//! Allocating the next ticket id is TUR-102's, because doing it safely needs
//! create-new semantics that `write_atomic` does not have.

use std::path::{Path, PathBuf};

use crate::meeting::Meeting;
use crate::ticket::Ticket;
use crate::transcript::{self, Transcript};
use crate::{Error, MEETING_FILE, NOTES_FILE, Problem, TICKETS_DIR, TRANSCRIPT_FILE, notes};

/// A problem, and the file it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileProblem {
    /// Relative to the meeting folder, e.g. `meeting.md` or
    /// `tickets/TICK-0003.md`, always with `/` separators.
    pub file: String,
    pub problem: Problem,
}

/// Everything in one meeting folder.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingFolder {
    /// The folder name, e.g. `2026-09-01-1430-standup`.
    pub id: String,
    pub path: PathBuf,
    /// `None` until an agent wraps the meeting up.
    pub meeting: Option<Meeting>,
    /// `None` when `transcript.md` does not exist.
    pub transcript: Option<Transcript>,
    /// Empty when `notes.md` does not exist, or when it cannot be read (then
    /// it is also named in `problems`).
    pub notes: String,
    /// Every `tickets/*.md`, sorted by file name.
    pub tickets: Vec<Ticket>,
    /// Every problem from every file above, tagged with its file.
    pub problems: Vec<FileProblem>,
}

impl MeetingFolder {
    /// Should the UI show the "needs attention" badge?
    pub fn needs_attention(&self) -> bool {
        !self.problems.is_empty()
    }
}

/// Resolve a meeting id to its folder under `root`, refusing anything that
/// is not a plain folder name ([`Error::BadId`]).
pub fn meeting_dir(root: &Path, id: &str) -> Result<PathBuf, Error> {
    if crate::is_plain_name(id) {
        Ok(root.join(id))
    } else {
        Err(Error::BadId(id.to_string()))
    }
}

/// Load one meeting folder. `Err` only when `dir` itself cannot be read;
/// every file inside it that is missing, malformed or unreadable becomes a
/// field left empty plus a [`FileProblem`], never an `Err`.
pub fn load(dir: &Path) -> Result<MeetingFolder, Error> {
    // Listing the folder is the one check that may fail the load: if this
    // works, everything after it is per-file and gets recorded, not raised.
    std::fs::read_dir(dir)?;

    let id = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut problems = Vec::new();

    let meeting = match Meeting::read(&dir.join(MEETING_FILE)) {
        Ok(Some(meeting)) => {
            tag(&mut problems, MEETING_FILE, &meeting.problems);
            Some(meeting)
        }
        Ok(None) => None,
        Err(error) => {
            unreadable(&mut problems, MEETING_FILE, &error);
            None
        }
    };

    let transcript = match transcript::read(&dir.join(TRANSCRIPT_FILE)) {
        Ok(Some(transcript)) => {
            tag(&mut problems, TRANSCRIPT_FILE, &transcript.problems);
            Some(transcript)
        }
        Ok(None) => None,
        Err(error) => {
            unreadable(&mut problems, TRANSCRIPT_FILE, &error);
            None
        }
    };

    let notes = notes::read(dir).unwrap_or_else(|error| {
        unreadable(&mut problems, NOTES_FILE, &error);
        String::new()
    });

    let tickets = load_tickets(&dir.join(TICKETS_DIR), &mut problems);

    Ok(MeetingFolder {
        id,
        path: dir.to_path_buf(),
        meeting,
        transcript,
        notes,
        tickets,
        problems,
    })
}

/// Every `*.md` in a meeting's `tickets/` folder, sorted by file name.
///
/// A missing folder is the normal "no tickets yet" case. Dotfiles are skipped
/// (that is where [`crate::write_atomic`] puts its temp files) and so is
/// anything that is not `.md`. A ticket that cannot be read at all is left out
/// of the list but still named in `problems`, so the badge shows.
fn load_tickets(tickets_dir: &Path, problems: &mut Vec<FileProblem>) -> Vec<Ticket> {
    let entries = match std::fs::read_dir(tickets_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            unreadable(problems, TICKETS_DIR, &Error::Io(error));
            return Vec::new();
        }
    };

    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') && name.ends_with(".md") {
                    names.push(name);
                }
            }
            Err(error) => unreadable(problems, TICKETS_DIR, &Error::Io(error)),
        }
    }
    names.sort();

    let mut tickets = Vec::with_capacity(names.len());
    for name in names {
        // Built by hand rather than from `Path::display` so the UI sees the
        // same `tickets/TICK-0001.md` on every OS (SPEC §8.2).
        let file = format!("{TICKETS_DIR}/{name}");
        match Ticket::read(&tickets_dir.join(&name)) {
            Ok(ticket) => {
                tag(problems, &file, &ticket.problems);
                tickets.push(ticket);
            }
            Err(error) => unreadable(problems, &file, &error),
        }
    }
    tickets
}

/// Attach `file` to each of one file's problems.
fn tag(problems: &mut Vec<FileProblem>, file: &str, found: &[Problem]) {
    problems.extend(found.iter().map(|problem| FileProblem {
        file: file.to_string(),
        problem: problem.clone(),
    }));
}

/// Record a file that could not be read at all (permission denied, a folder
/// where a file should be, …) as a problem instead of failing the load.
fn unreadable(problems: &mut Vec<FileProblem>, file: &str, error: &Error) {
    // `Error::Io`'s own message is the generic "could not read or write the
    // meetings folder"; the underlying OS error is the part worth showing.
    let detail = match error {
        Error::Io(io) => io.to_string(),
        other => other.to_string(),
    };
    problems.push(FileProblem {
        file: file.to_string(),
        problem: Problem::Unreadable { detail },
    });
}

/// Load every meeting folder under `root`, newest first (folder names start
/// with `YYYY-MM-DD-HHMM`, so a reverse name sort is a reverse date sort).
///
/// Skips dot-folders (`.app` holds config and the derived index) and plain
/// files. A folder that fails to load is logged with `tracing::warn!` and
/// skipped rather than failing the scan. A missing `root` is an empty list —
/// before the first recording it never exists.
pub fn scan(root: &Path) -> Result<Vec<MeetingFolder>, Error> {
    let mut folders = Vec::new();
    for dir in meeting_dirs(root)? {
        match load(&dir) {
            Ok(folder) => folders.push(folder),
            Err(error) => {
                tracing::warn!(path = %dir.display(), %error, "skipping a meeting folder that could not be read");
            }
        }
    }
    folders.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(folders)
}

/// Every meeting folder directly under `root`, unsorted: directories only,
/// dot-folders skipped. A missing `root` has none.
pub fn meeting_dirs(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        // `.app` holds config, models, logs and the derived index — it is not
        // a meeting. Skipping every dot-entry also covers `.DS_Store` and
        // whatever else the OS or a sync client leaves behind.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder under the OS temp dir, removed on drop. Named per test
    /// so tests running in parallel never share one.
    #[allow(dead_code)] // field 1 keeps the directory alive until drop
    struct Scratch(PathBuf, tempfile::TempDir);

    impl Scratch {
        fn new(name: &str) -> Self {
            let guard = tempfile::Builder::new()
                .prefix(&format!("meet-ai-store-folder-{name}-"))
                .tempdir()
                .unwrap();
            Self(guard.path().to_path_buf(), guard)
        }

        /// Create an empty file at `rel` (components joined with
        /// `Path::join`), making parent folders as needed.
        fn touch(&self, rel: &[&str]) {
            let path = rel.iter().fold(self.0.clone(), |p, c| p.join(c));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
    }

    #[test]
    fn a_meeting_id_cannot_escape_the_root() {
        let root = Path::new("meetings");
        for hostile in ["..", "../../etc", "/etc/passwd", ".app", "", "a/b", "a\\b"] {
            assert!(
                matches!(meeting_dir(root, hostile), Err(Error::BadId(id)) if id == hostile),
                "{hostile:?} must be refused"
            );
        }
        assert_eq!(
            meeting_dir(root, "2026-09-01-1430-standup").unwrap(),
            root.join("2026-09-01-1430-standup")
        );
    }

    #[test]
    fn a_missing_root_scans_as_no_meetings() {
        let scratch = Scratch::new("missing-root");
        assert!(scan(&scratch.0.join("never-created")).unwrap().is_empty());
    }

    #[test]
    fn scan_skips_dot_folders_and_plain_files_and_lists_newest_first() {
        let scratch = Scratch::new("scan-skips");
        for id in [
            "2026-09-01-1430-standup",
            "2026-09-03-0900-planning",
            "2026-09-02-1000-retro",
        ] {
            std::fs::create_dir(scratch.0.join(id)).unwrap();
        }
        scratch.touch(&[".app", "config.jsonc"]);
        scratch.touch(&[".DS_Store"]);
        scratch.touch(&["2026-09-04-1200-not-a-folder.md"]);

        let ids: Vec<_> = scan(&scratch.0)
            .unwrap()
            .into_iter()
            .map(|f| f.id)
            .collect();
        assert_eq!(
            ids,
            [
                "2026-09-03-0900-planning",
                "2026-09-02-1000-retro",
                "2026-09-01-1430-standup"
            ]
        );
    }

    #[test]
    fn an_empty_meeting_folder_loads_with_nothing_in_it_and_nothing_wrong() {
        let scratch = Scratch::new("empty-folder");
        let dir = scratch.0.join("2026-09-01-1430-standup");
        std::fs::create_dir(&dir).unwrap();

        let folder = load(&dir).unwrap();
        assert_eq!(folder.id, "2026-09-01-1430-standup");
        assert_eq!(folder.path, dir);
        assert!(folder.meeting.is_none());
        assert!(folder.transcript.is_none());
        assert_eq!(folder.notes, "");
        assert!(folder.tickets.is_empty());
        assert!(!folder.needs_attention());
    }

    #[test]
    fn a_missing_meeting_folder_is_an_error() {
        let scratch = Scratch::new("missing-folder");
        assert!(load(&scratch.0.join("2026-09-01-1430-standup")).is_err());
    }

    #[test]
    fn a_file_that_cannot_be_read_is_flagged_and_does_not_stop_the_load() {
        // A folder where a file should be is an I/O error on every OS, which
        // stands in for "permission denied" without any platform-specific
        // chmod (SPEC §8.2).
        let scratch = Scratch::new("unreadable");
        let dir = scratch.0.join("2026-09-01-1430-standup");
        std::fs::create_dir_all(dir.join(MEETING_FILE)).unwrap();
        std::fs::create_dir_all(dir.join(TICKETS_DIR).join("TICK-0002.md")).unwrap();
        std::fs::write(dir.join(NOTES_FILE), "still here").unwrap();
        std::fs::write(
            dir.join(TICKETS_DIR).join("TICK-0001.md"),
            "---\nid: TICK-0001\ntitle: Fine\nstatus: open\n---\n",
        )
        .unwrap();

        let folder = load(&dir).unwrap();
        assert!(folder.meeting.is_none());
        assert_eq!(folder.notes, "still here");
        assert!(
            folder
                .tickets
                .iter()
                .any(|t| t.id().as_deref() == Some("TICK-0001"))
        );
        let unreadable: Vec<_> = folder
            .problems
            .iter()
            .filter(|p| matches!(p.problem, Problem::Unreadable { .. }))
            .map(|p| p.file.as_str())
            .collect();
        assert_eq!(unreadable, ["meeting.md", "tickets/TICK-0002.md"]);
    }

    #[test]
    fn ticket_problems_name_the_ticket_with_forward_slashes() {
        let scratch = Scratch::new("ticket-paths");
        let dir = scratch.0.join("2026-09-01-1430-standup");
        std::fs::create_dir_all(dir.join(TICKETS_DIR)).unwrap();
        std::fs::write(
            dir.join(TICKETS_DIR).join("TICK-0002.md"),
            "no frontmatter\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(TICKETS_DIR).join("TICK-0001.md"),
            "---\nid: TICK-0001\ntitle: Fine\nstatus: open\n---\n",
        )
        .unwrap();

        let folder = load(&dir).unwrap();
        let ids: Vec<_> = folder.tickets.iter().map(|t| t.id()).collect();
        assert_eq!(ids, [Some("TICK-0001".to_string()), None]);
        assert!(folder.problems.contains(&FileProblem {
            file: "tickets/TICK-0002.md".into(),
            problem: Problem::NoFrontmatter,
        }));
        assert!(
            folder
                .problems
                .iter()
                .all(|p| p.file == "tickets/TICK-0002.md")
        );
    }
}
