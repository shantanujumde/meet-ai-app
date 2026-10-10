//! What the meeting list needs of each folder, without loading it (TUR-166).
//!
//! [`super::load`] parses all of `transcript.md`, reads every ticket and
//! collects every problem. A list row only needs `meeting.md` (title,
//! attendees, the notes switch, whether notes are written), whether
//! `notes.md` has text, and the transcript's line count and last timestamp.
//! Each summary is kept in a [`SummaryCache`] and made again only when one of
//! those three files changed size or modified time (or changed so recently
//! that a second change could hide in the same clock tick), so a list refresh reads
//! only the folders that changed (the ones a watcher event names) and no
//! transcript at all while nothing changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::meeting::Meeting;
use crate::stamp::Stamps;
use crate::transcript::{self, Stats};
use crate::{Error, MEETING_FILE, NOTES_FILE, TRANSCRIPT_FILE, notes};

/// One meeting folder, as much as a list row shows.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderSummary {
    /// The folder name, e.g. `2026-09-01-1430-standup`.
    pub id: String,
    pub path: PathBuf,
    /// `None` when there is no `meeting.md`, or it could not be read.
    pub meeting: Option<Meeting>,
    /// `None` when `transcript.md` does not exist.
    pub transcript: Option<Stats>,
    /// `notes.md` exists, is UTF-8 and is not empty.
    pub has_notes: bool,
}

impl From<&super::MeetingFolder> for FolderSummary {
    /// The same summary from a folder already loaded in full.
    fn from(folder: &super::MeetingFolder) -> Self {
        Self {
            id: folder.id.clone(),
            path: folder.path.clone(),
            meeting: folder.meeting.clone(),
            transcript: folder.transcript.as_ref().map(|t| Stats {
                line_count: t.lines.len(),
                last_time: t.lines.last().map(|line| line.time.clone()),
            }),
            has_notes: !folder.notes.is_empty(),
        }
    }
}

fn stamps(dir: &Path) -> Option<Stamps> {
    let paths = [MEETING_FILE, NOTES_FILE, TRANSCRIPT_FILE].map(|name| dir.join(name));
    Stamps::of(&paths.each_ref().map(PathBuf::as_path)).ok()
}

/// Folder summaries kept between list refreshes, by folder path. A folder
/// whose three files are unchanged is not read again, so a refresh after a
/// notes save reads that one folder's files and only `stat`s the rest.
/// Shared across threads; a poisoned lock is taken as is, because a
/// half-updated map only costs one extra read.
#[derive(Debug, Default)]
pub struct SummaryCache {
    entries: Mutex<HashMap<PathBuf, (Stamps, FolderSummary)>>,
}

impl SummaryCache {
    /// Forget every folder not in `keep`, so deleted meetings do not pile up.
    fn retain(&self, keep: &[PathBuf]) {
        self.lock().retain(|path, _| keep.contains(path));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, (Stamps, FolderSummary)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Summarise one meeting folder, from `cache` when none of its files changed.
/// `Err` only when `dir` is not a readable folder; a file that cannot be read
/// is left out of the summary, as [`super::load`] leaves it out of the folder.
pub fn load_summary(dir: &Path, cache: &SummaryCache) -> Result<FolderSummary, Error> {
    if !std::fs::metadata(dir)?.is_dir() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "a meeting is a folder",
        )));
    }
    let stamps = stamps(dir);
    if let Some(stamps) = &stamps
        && let Some((known, summary)) = cache.lock().get(dir)
        && known.still(stamps)
    {
        return Ok(summary.clone());
    }
    let summary = read_summary(dir);
    match stamps {
        Some(stamps) => {
            cache
                .lock()
                .insert(dir.to_path_buf(), (stamps, summary.clone()));
        }
        None => {
            cache.lock().remove(dir);
        }
    }
    Ok(summary)
}

/// Read the three files a summary is made from.
fn read_summary(dir: &Path) -> FolderSummary {
    let id = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meeting = Meeting::read(&dir.join(MEETING_FILE)).ok().flatten();
    let transcript = transcript::stats(&dir.join(TRANSCRIPT_FILE)).unwrap_or_else(|error| {
        tracing::warn!(path = %dir.display(), %error, "could not read a transcript for the meeting list");
        None
    });
    let has_notes = notes::read(dir).is_ok_and(|text| !text.is_empty());
    FolderSummary {
        id,
        path: dir.to_path_buf(),
        meeting,
        transcript,
        has_notes,
    }
}

/// [`load_summary`] for every meeting folder under `root`, newest first, with
/// the same skipping rules as [`super::scan`].
pub fn scan_summaries(root: &Path, cache: &SummaryCache) -> Result<Vec<FolderSummary>, Error> {
    let dirs = super::meeting_dirs(root)?;
    let mut summaries = Vec::with_capacity(dirs.len());
    for dir in &dirs {
        match load_summary(dir, cache) {
            Ok(summary) => summaries.push(summary),
            Err(error) => {
                tracing::warn!(path = %dir.display(), %error, "skipping a meeting folder that could not be read");
            }
        }
    }
    cache.retain(&dirs);
    summaries.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "2026-09-01-1430-standup";

    fn meeting(root: &Path) -> PathBuf {
        let dir = root.join(ID);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(MEETING_FILE),
            "---\nid: x\ntitle: Platform Standup\n---\n\n## Summary\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(TRANSCRIPT_FILE),
            "[00:00:01] You: one\n[00:00:05] Others: two\n",
        )
        .unwrap();
        std::fs::write(dir.join(NOTES_FILE), "mine").unwrap();
        for name in [MEETING_FILE, TRANSCRIPT_FILE, NOTES_FILE] {
            age(&dir.join(name));
        }
        dir
    }

    /// Move a file's time a minute back, so the cache trusts its stamp.
    fn age(path: &Path) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
    }

    #[test]
    fn a_summary_matches_the_full_load() {
        let root = tempfile::tempdir().unwrap();
        let dir = meeting(root.path());
        let full = FolderSummary::from(&super::super::load(&dir).unwrap());
        let light = load_summary(&dir, &SummaryCache::default()).unwrap();
        assert_eq!(light, full);
        assert_eq!(light.transcript.as_ref().unwrap().line_count, 2);
        assert!(light.has_notes);
    }

    /// TUR-166's "a list refresh does not parse transcripts": a transcript
    /// whose size and time are unchanged is not read again, so a different
    /// text under the same stamp still shows the old count.
    #[test]
    fn an_unchanged_transcript_is_not_read_again() {
        let root = tempfile::tempdir().unwrap();
        let path = meeting(root.path()).join(TRANSCRIPT_FILE);
        let cache = SummaryCache::default();
        let first = scan_summaries(root.path(), &cache).unwrap();
        assert_eq!(first[0].transcript.as_ref().unwrap().line_count, 2);

        // Same length, same modified time, but unparseable text: a read would
        // count 0 lines.
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let same_len = "x".repeat(std::fs::metadata(&path).unwrap().len() as usize);
        std::fs::write(&path, same_len).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let again = scan_summaries(root.path(), &cache).unwrap();
        assert_eq!(again[0].transcript.as_ref().unwrap().line_count, 2);

        // A grown transcript is read again.
        std::fs::write(
            &path,
            "[00:00:01] You: one\n[00:00:05] Others: two\n[00:00:09] You: three\n",
        )
        .unwrap();
        let grown = scan_summaries(root.path(), &cache).unwrap();
        let stats = grown[0].transcript.as_ref().unwrap();
        assert_eq!(stats.line_count, 3);
        assert_eq!(stats.last_time.as_deref(), Some("00:00:09"));
    }

    /// A notes save re-reads that folder, so the row's notes flag follows.
    #[test]
    fn a_changed_notes_file_is_read_again() {
        let root = tempfile::tempdir().unwrap();
        let dir = meeting(root.path());
        let cache = SummaryCache::default();
        assert!(scan_summaries(root.path(), &cache).unwrap()[0].has_notes);
        std::fs::write(dir.join(NOTES_FILE), "").unwrap();
        assert!(!scan_summaries(root.path(), &cache).unwrap()[0].has_notes);
    }

    #[test]
    fn a_deleted_meeting_leaves_the_cache() {
        let root = tempfile::tempdir().unwrap();
        let dir = meeting(root.path());
        let cache = SummaryCache::default();
        scan_summaries(root.path(), &cache).unwrap();
        assert_eq!(cache.lock().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(scan_summaries(root.path(), &cache).unwrap().is_empty());
        assert!(cache.lock().is_empty());
    }

    #[test]
    fn missing_files_summarise_as_empty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(ID);
        std::fs::create_dir_all(&dir).unwrap();
        let summary = load_summary(&dir, &SummaryCache::default()).unwrap();
        assert!(summary.meeting.is_none());
        assert!(summary.transcript.is_none());
        assert!(!summary.has_notes);
        assert!(load_summary(&root.path().join("gone"), &SummaryCache::default()).is_err());
    }
}
