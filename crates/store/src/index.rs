//! `index.db`: the derived full-text index (SPEC §3.6, TUR-101).
//!
//! **L7 — markdown is the truth.** Everything in this file is rebuilt from the
//! meetings folder. The database lives at `<root>/.app/index.db`, is safe to
//! delete at any moment, and is rebuilt by [`Index::open`] when it is missing,
//! unreadable or written by a different [`SCHEMA_VERSION`]. A rescan of the same
//! folder always gives the same rows in the same order, so a search gives the
//! same answer before and after the file is deleted.
//!
//! What is searchable: each transcript line (with its timestamp and speaker),
//! the meeting title, the `meeting.md` sections, the user's notes and every
//! ticket's title and body. The last four have no timestamp.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension as _, params};

use crate::folder::{self, MeetingFolder};
use crate::{Error, TICKETS_DIR, is_plain_name};
use meeting_format::layout::{MEETING_FILE, NOTES_FILE, TRANSCRIPT_FILE, app_dir};

mod catch_up;
mod titled;
pub use titled::{IndexedMeeting, same_title_key};

/// Bump when the tables below change. A file with another number is dropped
/// and rebuilt, never migrated: the markdown is the truth, so rebuilding is
/// always safe. 2: `tickets` keyed by (meeting, ticket) (TUR-154).
pub const SCHEMA_VERSION: i32 = 2;

/// Markers [`Hit::snippet`] puts around each matched word.
pub const MATCH_START: char = '\u{ab}';
pub const MATCH_END: char = '\u{bb}';

/// The most hits one search returns.
const MAX_HITS: i64 = 50;

const INDEX_FILE: &str = "index.db";

/// Rows for the tables in SPEC §3.6, minus `transcript_vec` (v1.1).
const SCHEMA: &str = "
CREATE TABLE meetings(
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    date TEXT,
    duration_sec INTEGER,
    path TEXT NOT NULL,
    has_analysis INTEGER NOT NULL,
    mtime INTEGER NOT NULL
);
CREATE TABLE tickets(
    id TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT,
    assignee TEXT,
    estimate TEXT,
    synced_to TEXT,
    path TEXT NOT NULL,
    mtime INTEGER NOT NULL,
    PRIMARY KEY (meeting_id, id)
);
CREATE VIRTUAL TABLE transcript_fts USING fts5(
    meeting_id UNINDEXED, ts UNINDEXED, speaker UNINDEXED, text
);
";

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub meeting_id: String,
    pub title: String,
    pub date: Option<String>,
    /// The matching text, each matched word wrapped in [`MATCH_START`] and
    /// [`MATCH_END`].
    pub snippet: String,
    /// `HH:MM:SS` into the recording; `None` for a title, notes or ticket hit.
    pub timestamp: Option<String>,
}

/// The open index.
pub struct Index {
    conn: Connection,
}

/// Where the index for the meetings folder `root` lives.
pub fn index_path(root: &Path) -> PathBuf {
    app_dir(root).join(INDEX_FILE)
}

impl Index {
    /// Open the index for `root`, building it from the markdown first when the
    /// file is missing, damaged or from another schema version. A file that is
    /// kept is not compared with the folder; [`Index::catch_up`] does that.
    pub fn open(root: &Path) -> Result<Self, Error> {
        let path = index_path(root);
        if let Some(index) = Self::try_open(&path) {
            return Ok(index);
        }
        remove_files(&path);
        let mut index = Self::create(&path)?;
        index.rescan(root)?;
        Ok(index)
    }

    /// The existing file, if it opens and has the current schema.
    fn try_open(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let conn = Connection::open(path).ok()?;
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .ok()?;
        if version != SCHEMA_VERSION {
            return None;
        }
        // A file that opened can still be garbage past its header.
        conn.query_row("SELECT count(*) FROM meetings", [], |row| {
            row.get::<_, i64>(0)
        })
        .ok()?;
        conn.query_row("SELECT count(*) FROM transcript_fts", [], |row| {
            row.get::<_, i64>(0)
        })
        .ok()?;
        Some(Self { conn })
    }

    fn create(path: &Path) -> Result<Self, Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).map_err(index_error)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            .map_err(index_error)?;
        conn.execute_batch(SCHEMA).map_err(index_error)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(index_error)?;
        Ok(Self { conn })
    }

    /// Throw away every row and index the whole folder again.
    pub fn rescan(&mut self, root: &Path) -> Result<(), Error> {
        let folders = folder::scan(root)?;
        let tx = self.conn.transaction().map_err(index_error)?;
        tx.execute_batch("DELETE FROM meetings; DELETE FROM tickets; DELETE FROM transcript_fts;")
            .map_err(index_error)?;
        for folder in &folders {
            insert_folder(&tx, folder, folder_mtime(&folder.path))?;
        }
        tx.commit().map_err(index_error)
    }

    /// Bring the index up to date for the files in `paths` (what the watcher
    /// reports). Only meetings whose files changed since they were indexed are
    /// read again. Returns how many meetings were re-indexed or removed.
    ///
    /// `paths` may be under `root` as given or under its canonical form: the
    /// watcher reports real paths, so a symlinked root (an iCloud or Dropbox
    /// `~/Meetings`) or macOS's `/var` (really `/private/var`) still matches
    /// (TUR-152).
    pub fn update(&mut self, root: &Path, paths: &[PathBuf]) -> Result<usize, Error> {
        // dunce: std's Windows canonical form `\\?\C:\…` never prefixes the
        // `C:\…` paths notify reports (as in `watcher.rs`).
        let canonical = dunce::canonicalize(root).ok();
        let ids: BTreeSet<String> = paths
            .iter()
            .filter_map(|path| {
                meeting_id_of(root, path).or_else(|| {
                    canonical
                        .as_deref()
                        .and_then(|real| meeting_id_of(real, path))
                })
            })
            .collect();
        let tx = self.conn.transaction().map_err(index_error)?;
        let mut changed = 0;
        for id in ids {
            changed += usize::from(reindex(&tx, root, &id, false)?);
        }
        tx.commit().map_err(index_error)?;
        Ok(changed)
    }

    /// Read meeting `meeting_id` again now, however its files' times compare
    /// with the index's (TUR-107).
    ///
    /// For the app's own writes to `meeting.md`: the watcher skips those
    /// ([`crate::watcher::SelfWrites`]), so without this a title the agent
    /// wrote would not be searchable, nor found by the pre-meeting brief
    /// ([`Index::meetings_titled`]), until the next rescan. A folder that is
    /// gone is dropped from the index.
    ///
    /// # Errors
    ///
    /// [`Error::BadId`] for an id that is not a plain folder name, and
    /// [`Error::Index`] when the database cannot be written.
    pub fn refresh_meeting(&mut self, root: &Path, meeting_id: &str) -> Result<(), Error> {
        folder::meeting_dir(root, meeting_id)?;
        let tx = self.conn.transaction().map_err(index_error)?;
        reindex(&tx, root, meeting_id, true)?;
        tx.commit().map_err(index_error)
    }

    /// Find the transcript lines, notes, summaries and tickets matching every
    /// word of `query` (the last word also matches as a prefix, so results
    /// appear while typing). Best match first; ties break by meeting then
    /// position, so the order is stable. A blank query finds nothing.
    pub fn search(&self, query: &str) -> Result<Vec<Hit>, Error> {
        let Some(fts_query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut statement = self
            .conn
            .prepare(
                "SELECT f.meeting_id, m.title, m.date, f.ts,
                        snippet(transcript_fts, 3, ?2, ?3, '…', 12)
                 FROM transcript_fts f JOIN meetings m ON m.id = f.meeting_id
                 WHERE transcript_fts MATCH ?1
                 ORDER BY bm25(transcript_fts), f.meeting_id, f.rowid
                 LIMIT ?4",
            )
            .map_err(index_error)?;
        let rows = statement
            .query_map(
                params![
                    fts_query,
                    MATCH_START.to_string(),
                    MATCH_END.to_string(),
                    MAX_HITS
                ],
                |row| {
                    Ok(Hit {
                        meeting_id: row.get(0)?,
                        title: row.get(1)?,
                        date: row.get(2)?,
                        timestamp: row.get(3)?,
                        snippet: row.get(4)?,
                    })
                },
            )
            .map_err(index_error)?;
        rows.collect::<Result<_, _>>().map_err(index_error)
    }
}

/// Turn what the user typed into an FTS5 query: every word quoted (so `-`, `:`
/// and `"` are plain text, never syntax), the last one a prefix.
fn fts_query(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect();
    let last = words.len().checked_sub(1)?;
    Some(
        words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                if i == last {
                    format!("{word}*")
                } else {
                    word.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The meeting folder a changed path belongs to, if it is under one.
fn meeting_id_of(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let Some(Component::Normal(first)) = relative.components().next() else {
        return None;
    };
    let id = first.to_str()?;
    is_plain_name(id).then(|| id.to_owned())
}

/// Read one meeting into the index again, or drop it when its folder is gone.
/// Unless `force`, a meeting whose files are no newer than its row is left as
/// it is. `true` when its rows changed.
fn reindex(
    tx: &rusqlite::Transaction<'_>,
    root: &Path,
    id: &str,
    force: bool,
) -> Result<bool, Error> {
    let dir = root.join(id);
    if !dir.is_dir() {
        return delete_meeting(tx, id);
    }
    let mtime = folder_mtime(&dir);
    if !force {
        let known: Option<i64> = tx
            .query_row("SELECT mtime FROM meetings WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(index_error)?;
        if known == Some(mtime) {
            return Ok(false);
        }
    }
    let removed = delete_meeting(tx, id)?;
    // A folder that cannot be read right now (deleted mid-batch) is treated
    // like a deleted one; the next event brings it back.
    if let Ok(folder) = folder::load(&dir) {
        insert_folder(tx, &folder, mtime)?;
        return Ok(true);
    }
    Ok(removed)
}

/// Delete every row of one meeting. `true` if it had any.
fn delete_meeting(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<bool, Error> {
    let removed = tx
        .execute("DELETE FROM meetings WHERE id = ?1", [id])
        .map_err(index_error)?;
    tx.execute("DELETE FROM tickets WHERE meeting_id = ?1", [id])
        .map_err(index_error)?;
    tx.execute("DELETE FROM transcript_fts WHERE meeting_id = ?1", [id])
        .map_err(index_error)?;
    Ok(removed > 0)
}

/// Add one loaded meeting folder to all three tables.
fn insert_folder(
    tx: &rusqlite::Transaction<'_>,
    folder: &MeetingFolder,
    mtime: i64,
) -> Result<(), Error> {
    let meeting = folder.meeting.as_ref();
    let title = meeting
        .and_then(|m| m.title())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| folder.id.clone());
    tx.execute(
        "INSERT INTO meetings(id, title, date, duration_sec, path, has_analysis, mtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            folder.id,
            title,
            meeting.and_then(|m| m.date()),
            meeting.and_then(|m| m.duration_sec()),
            folder.id,
            meeting.is_some_and(|m| m.is_analyzed()),
            mtime,
        ],
    )
    .map_err(index_error)?;

    let text_row = |ts: Option<&str>, speaker: &str, text: &str| -> Result<(), Error> {
        if text.trim().is_empty() {
            return Ok(());
        }
        tx.execute(
            "INSERT INTO transcript_fts(meeting_id, ts, speaker, text) VALUES (?1, ?2, ?3, ?4)",
            params![folder.id, ts, speaker, text],
        )
        .map_err(index_error)?;
        Ok(())
    };

    text_row(None, "title", &title)?;
    if let Some(meeting) = meeting {
        for section in &meeting.sections {
            text_row(None, "meeting", &section.body)?;
        }
    }
    text_row(None, "notes", &folder.notes)?;
    if let Some(transcript) = &folder.transcript {
        for line in &transcript.lines {
            text_row(Some(&line.time), line.speaker.label(), &line.text)?;
        }
    }

    for ticket in &folder.tickets {
        let Some(id) = ticket.id() else { continue };
        let file = format!("{TICKETS_DIR}/{id}.md");
        let ticket_title = ticket.title().unwrap_or_else(|| id.clone());
        tx.execute(
            "INSERT OR REPLACE INTO tickets(id, meeting_id, title, status, assignee, estimate,
                                            synced_to, path, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                folder.id,
                ticket_title,
                ticket.status().map(|s| s.as_str()),
                ticket.assignee(),
                ticket.estimate(),
                ticket.synced_to(),
                file,
                modified_ms(&folder.path.join(&file)),
            ],
        )
        .map_err(index_error)?;
        text_row(None, "ticket", &ticket_title)?;
        text_row(None, "ticket", &ticket.body)?;
    }
    Ok(())
}

/// The newest change among the files the index reads from one meeting folder.
/// The `tickets/` folder counts too, so deleting a ticket moves it, and so
/// does the meeting folder itself, so deleting `notes.md` (or any file that
/// was not the newest) moves it as well (TUR-152).
fn folder_mtime(dir: &Path) -> i64 {
    let tickets = dir.join(TICKETS_DIR);
    let mut newest = [MEETING_FILE, TRANSCRIPT_FILE, NOTES_FILE]
        .iter()
        .map(|name| modified_ms(&dir.join(name)))
        .chain([modified_ms(&tickets), modified_ms(dir)])
        .max()
        .unwrap_or(0);
    if let Ok(entries) = std::fs::read_dir(&tickets) {
        for entry in entries.flatten() {
            newest = newest.max(modified_ms(&entry.path()));
        }
    }
    newest
}

/// Last-modified time in milliseconds since the epoch; 0 when missing.
fn modified_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Remove the database and its WAL side files, ignoring any that are missing.
fn remove_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        std::fs::remove_file(PathBuf::from(name)).ok();
    }
}

fn index_error(error: rusqlite::Error) -> Error {
    Error::Index(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_quoted_and_the_last_is_a_prefix() {
        assert_eq!(
            fts_query("budget plan").as_deref(),
            Some("\"budget\" \"plan\"*")
        );
        assert_eq!(fts_query("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        assert_eq!(fts_query("   "), None);
    }

    #[test]
    fn only_paths_inside_a_meeting_folder_name_a_meeting() {
        let root = Path::new("/m");
        assert_eq!(
            meeting_id_of(root, Path::new("/m/2026-09-01-standup/notes.md")).as_deref(),
            Some("2026-09-01-standup")
        );
        assert_eq!(meeting_id_of(root, Path::new("/m/.app/index.db")), None);
        assert_eq!(meeting_id_of(root, Path::new("/m")), None);
        assert_eq!(
            meeting_id_of(root, Path::new("/elsewhere/x/notes.md")),
            None
        );
    }

    fn ticket_rows(index: &Index) -> Vec<(String, String)> {
        let mut stmt = index
            .conn
            .prepare("SELECT meeting_id, id FROM tickets ORDER BY meeting_id, id")
            .expect("prepare");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    }

    /// TUR-154: two meetings holding the same ticket id keep a row each, and
    /// dropping one meeting leaves the other's row.
    #[test]
    fn two_meetings_with_the_same_ticket_id_keep_a_row_each() {
        let root = tempfile::tempdir().expect("tempdir");
        let (a, b) = ("2026-10-01-1000-a", "2026-10-02-1000-b");
        for id in [a, b] {
            let tickets = root.path().join(id).join(TICKETS_DIR);
            std::fs::create_dir_all(&tickets).expect("tickets dir");
            crate::ticket::Ticket::new("TICK-0001", "Same number", id)
                .write(&tickets.join("TICK-0001.md"))
                .expect("ticket");
        }
        let mut index = Index::open(root.path()).expect("open");
        let row = |m: &str| (m.to_owned(), "TICK-0001".to_owned());
        assert_eq!(ticket_rows(&index), vec![row(a), row(b)]);

        std::fs::remove_dir_all(root.path().join(a)).expect("remove a");
        index.refresh_meeting(root.path(), a).expect("refresh");
        assert_eq!(ticket_rows(&index), vec![row(b)]);
    }
}
