//! TUR-101 gate: delete `index.db`, rescan, get exactly the same results.
//!
//! Runs over a copy of the `tests/fixtures/meetings/` folder (the same one
//! `fixture_folder.rs` uses), through the crate's public API only.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use store::index::{Hit, Index, index_path};

const STANDUP: &str = "2026-09-01-1430-standup";
const RETRO: &str = "2026-09-02-1000-retro";

/// A throwaway copy of the fixture root, removed on drop.
struct FixtureCopy {
    root: PathBuf,
    _guard: tempfile::TempDir,
}

impl FixtureCopy {
    fn new(name: &str) -> Self {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("meetings");
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-index-{name}-"))
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

const QUERIES: [&str; 8] = [
    "redis",
    "red",
    "integration tests",
    "load test",
    "sessions memory",
    "standup",
    "nothing-matches-this",
    "\"quoted\" AND -weird:syntax",
];

fn all_results(index: &Index) -> Vec<(String, Vec<Hit>)> {
    QUERIES
        .iter()
        .map(|query| ((*query).to_owned(), index.search(query).unwrap()))
        .collect()
}

#[test]
fn transcript_lines_are_found_with_their_timestamp() {
    let fixture = FixtureCopy::new("found");
    let index = Index::open(&fixture.root).unwrap();

    let hits = index.search("redis").unwrap();
    let line = hits
        .iter()
        .find(|hit| hit.timestamp.as_deref() == Some("00:14:22"))
        .expect("the 00:14:22 line mentions Redis");
    assert_eq!(line.meeting_id, STANDUP);
    assert!(line.snippet.contains("\u{ab}Redis\u{bb}"));

    let retro = index.search("failed").unwrap();
    assert!(!retro.is_empty());
    assert!(retro.iter().all(|hit| hit.meeting_id == RETRO));
    assert_eq!(retro[0].timestamp.as_deref(), Some("00:00:15"));
}

/// TUR-166, from a TUR-152 finding: "integration" finds nothing because the
/// only line that says it is `[00:00:09] Priya: …`, which SPEC §3.4 does not
/// parse (the fixture's deliberate bad speaker), so the meeting view does not
/// show it either. Lines that parse are indexed.
#[test]
fn a_line_with_a_speaker_the_spec_does_not_allow_is_not_indexed() {
    let fixture = FixtureCopy::new("unparsed");
    let index = Index::open(&fixture.root).unwrap();
    assert!(index.search("integration").unwrap().is_empty());
    let parsed = index.search("sprint").unwrap();
    assert!(
        parsed
            .iter()
            .any(|hit| hit.timestamp.as_deref() == Some("00:00:02"))
    );
}

#[test]
fn notes_are_found_without_a_timestamp() {
    let fixture = FixtureCopy::new("notes");
    let index = Index::open(&fixture.root).unwrap();
    let hits = index.search("load test").unwrap();
    assert!(hits.iter().any(|hit| hit.timestamp.is_none()));
}

#[test]
fn blank_and_syntax_only_queries_do_not_fail() {
    let fixture = FixtureCopy::new("blank");
    let index = Index::open(&fixture.root).unwrap();
    assert!(index.search("   ").unwrap().is_empty());
    index.search("\"").unwrap();
    index.search("NEAR(").unwrap();
}

#[test]
fn deleting_index_db_and_rescanning_gives_the_same_results() {
    let fixture = FixtureCopy::new("rebuild");
    let index = Index::open(&fixture.root).unwrap();
    let before = all_results(&index);
    assert!(before.iter().any(|(_, hits)| !hits.is_empty()));
    drop(index);

    let path = index_path(&fixture.root);
    assert!(path.is_file());
    fs::remove_file(&path).unwrap();

    let mut rebuilt = Index::open(&fixture.root).unwrap();
    assert!(path.is_file());
    assert_eq!(all_results(&rebuilt), before);

    // An explicit rescan of the live index changes nothing either.
    rebuilt.rescan(&fixture.root).unwrap();
    assert_eq!(all_results(&rebuilt), before);
}

#[test]
fn a_damaged_index_file_is_rebuilt() {
    let fixture = FixtureCopy::new("damaged");
    let before = all_results(&Index::open(&fixture.root).unwrap());

    fs::write(index_path(&fixture.root), b"this is not a database").unwrap();
    let index = Index::open(&fixture.root).unwrap();
    assert_eq!(all_results(&index), before);
}

#[test]
fn a_file_from_another_schema_version_is_rebuilt() {
    let fixture = FixtureCopy::new("version");
    let before = all_results(&Index::open(&fixture.root).unwrap());

    // `user_version` is a big-endian u32 at byte 60 of the SQLite header. The
    // first open already checkpointed and closed, so the header holds it.
    let path = index_path(&fixture.root);
    let mut bytes = fs::read(&path).unwrap();
    bytes[60..64].copy_from_slice(&99_u32.to_be_bytes());
    fs::write(&path, bytes).unwrap();

    let index = Index::open(&fixture.root).unwrap();
    assert_eq!(all_results(&index), before);
}

#[test]
fn an_edit_is_picked_up_incrementally_and_a_delete_removes_the_meeting() {
    let fixture = FixtureCopy::new("update");
    let mut index = Index::open(&fixture.root).unwrap();
    assert!(index.search("zebra").unwrap().is_empty());

    let notes = fixture.root.join(STANDUP).join("notes.md");
    fs::write(&notes, "# My notes\n\n- Ask about the zebra migration.\n").unwrap();
    // Make the change visible to the mtime check even on a coarse clock.
    fs::File::options()
        .write(true)
        .open(&notes)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();

    // The unchanged meeting is skipped: only the edited one is read again.
    let unchanged = fixture.root.join(RETRO).join("meeting.md");
    let touched = index.update(&fixture.root, &[notes, unchanged]).unwrap();
    assert_eq!(touched, 1);
    assert_eq!(index.search("zebra").unwrap()[0].meeting_id, STANDUP);

    // Same answer as a from-scratch rebuild.
    let incremental = all_results(&index);
    index.rescan(&fixture.root).unwrap();
    assert_eq!(all_results(&index), incremental);

    fs::remove_dir_all(fixture.root.join(STANDUP)).unwrap();
    let gone = fixture.root.join(STANDUP).join("notes.md");
    assert_eq!(index.update(&fixture.root, &[gone]).unwrap(), 1);
    assert!(index.search("zebra").unwrap().is_empty());
    assert!(index.search("redis").unwrap().is_empty());
}
