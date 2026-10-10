//! TUR-152: search sees the app's own writes, and edits made while the app was
//! closed.
//!
//! The watcher skips the files the app writes itself (`notes.md` saves, the
//! agent's `meeting.md` and tickets), so the app refreshes those meetings in
//! the index right after writing. A kept `index.db` is caught up with the
//! folder at launch by [`Index::catch_up`]. Runs over copies of
//! `tests/fixtures/`, through the crate's public API only.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use prompts::notes::Notes;
use store::agent_notes::{self, Analysis, AnalyzedBy};
use store::index::Index;
use store::watcher::SelfWrites;
use store::{NOTES_FILE, TRANSCRIPT_FILE};

const STANDUP: &str = "2026-09-01-1430-standup";
const RETRO: &str = "2026-09-02-1000-retro";

/// A throwaway copy of the fixture meetings root, removed on drop.
struct FixtureCopy {
    root: PathBuf,
    _guard: tempfile::TempDir,
}

impl FixtureCopy {
    fn new(name: &str) -> Self {
        let source = fixtures().join("meetings");
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-catch-up-{name}-"))
            .tempdir()
            .unwrap();
        let root = guard.path().to_path_buf();
        copy_dir(&source, &root);
        Self {
            root,
            _guard: guard,
        }
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), &dest).unwrap();
        }
    }
}

/// Move `path`'s modified time, so the index's time check sees the change
/// even on a coarse clock.
fn set_mtime(path: &Path, at: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn later() -> SystemTime {
    SystemTime::now() + Duration::from_secs(5)
}

fn hit_ids(index: &Index, query: &str) -> Vec<String> {
    index
        .search(query)
        .unwrap()
        .into_iter()
        .map(|hit| hit.meeting_id)
        .collect()
}

// ---------------------------------------------------------------------------
// The app's own writes

#[test]
fn notes_saved_in_the_app_are_found_once_the_meeting_is_refreshed() {
    let fixture = FixtureCopy::new("own-notes");
    let mut index = Index::open(&fixture.root).unwrap();

    store::notes::write(
        &fixture.root.join(STANDUP),
        "Ask about the zebra migration.",
    )
    .unwrap();
    assert!(
        index.search("zebra").unwrap().is_empty(),
        "the watcher skips our own write, so nothing has told the index yet"
    );

    index.refresh_meeting(&fixture.root, STANDUP).unwrap();
    let hits = index.search("zebra").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meeting_id, STANDUP);
    assert_eq!(hits[0].timestamp, None, "a notes hit has no timestamp");
}

#[test]
fn an_agent_notes_run_is_found_once_the_meeting_is_refreshed() {
    let root = tempfile::Builder::new()
        .prefix("meet-ai-store-catch-up-own-run-")
        .tempdir()
        .unwrap();
    let id = "2026-09-02-1000-meeting";
    let dir = root.path().join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(TRANSCRIPT_FILE),
        "[00:00:04] Others: Morning everyone, the release slipped.\n",
    )
    .unwrap();
    let mut index = Index::open(root.path()).unwrap();

    let notes =
        Notes::from_json(&fs::read_to_string(fixtures().join("notes").join("retro.json")).unwrap())
            .unwrap();
    let analysis = Analysis {
        by: AnalyzedBy::ClaudeCode,
        model: "opus".to_owned(),
        at: "2026-09-02T11:00:00+05:30".to_owned(),
    };
    let outcome =
        agent_notes::write(root.path(), id, &notes, &analysis, &SelfWrites::default()).unwrap();
    assert_eq!(outcome.written.len(), 2);
    assert!(index.search("staging").unwrap().is_empty());

    index.refresh_meeting(root.path(), id).unwrap();
    // The summary, a decision and both tickets are searchable now.
    assert!(hit_ids(&index, "database out of date").contains(&id.to_owned()));
    assert!(hit_ids(&index, "every Monday").contains(&id.to_owned()));
    assert!(hit_ids(&index, "staging refresh").contains(&id.to_owned()));
    assert!(hit_ids(&index, "staging age check").contains(&id.to_owned()));
    // And the pre-meeting brief finds the meeting by the agent's title (A20).
    let titled = index.meetings_titled("Release slip retro").unwrap();
    assert_eq!(titled.len(), 1);
    assert_eq!(titled[0].id, id);
}

// ---------------------------------------------------------------------------
// Catching up at launch

#[test]
fn a_kept_index_catches_up_with_edits_made_while_the_app_was_closed() {
    let fixture = FixtureCopy::new("offline");
    drop(Index::open(&fixture.root).unwrap());

    // While the app is closed: an edit, a new meeting, a deleted one.
    let notes = fixture.root.join(STANDUP).join(NOTES_FILE);
    fs::write(&notes, "# My notes\n\n- Ask about the zebra migration.\n").unwrap();
    set_mtime(&notes, later());
    let new_dir = fixture.root.join("2026-09-04-0900-sync");
    fs::create_dir_all(&new_dir).unwrap();
    fs::write(new_dir.join(NOTES_FILE), "Quokka budget review.\n").unwrap();
    fs::remove_dir_all(fixture.root.join(RETRO)).unwrap();

    let mut index = Index::open(&fixture.root).unwrap();
    assert!(
        index.search("zebra").unwrap().is_empty(),
        "a kept index.db is reused as it is"
    );
    assert!(hit_ids(&index, "flaky").contains(&RETRO.to_owned()));

    assert_eq!(index.catch_up(&fixture.root).unwrap(), 3);
    assert_eq!(hit_ids(&index, "zebra"), [STANDUP]);
    assert_eq!(hit_ids(&index, "quokka"), ["2026-09-04-0900-sync"]);
    assert!(!hit_ids(&index, "flaky").contains(&RETRO.to_owned()));

    // Nothing changed since: nothing is read again.
    assert_eq!(index.catch_up(&fixture.root).unwrap(), 0);

    // Same answers as a from-scratch rebuild.
    let queries = ["zebra", "quokka", "redis", "flaky", "standup"];
    let caught_up: Vec<_> = queries.iter().map(|q| index.search(q).unwrap()).collect();
    index.rescan(&fixture.root).unwrap();
    let rebuilt: Vec<_> = queries.iter().map(|q| index.search(q).unwrap()).collect();
    assert_eq!(caught_up, rebuilt);
}

#[test]
fn deleting_a_file_that_was_not_the_newest_is_caught_up_too() {
    let fixture = FixtureCopy::new("offline-delete");
    let dir = fixture.root.join(STANDUP);
    let notes = dir.join(NOTES_FILE);
    fs::write(&notes, "Kangaroo notes.\n").unwrap();
    // `notes.md` is older than the meeting's other files, so removing it does
    // not change the newest file time.
    set_mtime(&notes, SystemTime::now() - Duration::from_secs(3600));
    drop(Index::open(&fixture.root).unwrap());
    assert_eq!(
        hit_ids(&Index::open(&fixture.root).unwrap(), "kangaroo"),
        [STANDUP]
    );

    // Let the folder's own time move past the one the index holds.
    std::thread::sleep(Duration::from_millis(50));
    fs::remove_file(&notes).unwrap();

    let mut index = Index::open(&fixture.root).unwrap();
    assert_eq!(index.catch_up(&fixture.root).unwrap(), 1);
    assert!(index.search("kangaroo").unwrap().is_empty());
    assert!(hit_ids(&index, "redis").contains(&STANDUP.to_owned()));
}

#[test]
fn a_freshly_built_index_has_nothing_to_catch_up() {
    let fixture = FixtureCopy::new("fresh");
    let mut index = Index::open(&fixture.root).unwrap();
    assert_eq!(index.catch_up(&fixture.root).unwrap(), 0);
}
