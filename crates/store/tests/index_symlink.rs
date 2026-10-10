//! TUR-152: the index stays up to date when the meetings root is a symlink
//! (an iCloud or Dropbox `~/Meetings`, or macOS's `/var` -> `/private/var`).
//!
//! The watcher reports real (canonical) paths while the app holds the root as
//! configured, so `Index::update` must match either form. Unix only: making a
//! symlink on Windows needs a privilege CI does not have.
#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use store::NOTES_FILE;
use store::index::Index;

const STANDUP: &str = "2026-09-01-1430-standup";

/// `<tmp>/real` holding one meeting, and `<tmp>/link` pointing at it.
struct LinkedRoot {
    real: PathBuf,
    link: PathBuf,
    _guard: tempfile::TempDir,
}

impl LinkedRoot {
    fn new(name: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-symlink-{name}-"))
            .tempdir()
            .unwrap();
        let real = guard.path().join("real");
        let dir = real.join(STANDUP);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(NOTES_FILE), "Redis first.\n").unwrap();
        let link = guard.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        Self {
            real,
            link,
            _guard: guard,
        }
    }
}

fn write_later(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();
}

#[test]
fn a_change_reported_under_the_real_path_updates_an_index_opened_through_the_link() {
    let root = LinkedRoot::new("update");
    let mut index = Index::open(&root.link).unwrap();
    assert_eq!(index.search("redis").unwrap().len(), 1);

    let notes = root.link.join(STANDUP).join(NOTES_FILE);
    write_later(&notes, "Ask about the zebra migration.\n");
    // What the watcher hands over: the canonical form of the changed file.
    let reported = fs::canonicalize(&notes).unwrap();
    assert!(reported.starts_with(fs::canonicalize(&root.real).unwrap()));
    assert!(!reported.starts_with(&root.link));

    assert_eq!(index.update(&root.link, &[reported]).unwrap(), 1);
    let hits = index.search("zebra").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meeting_id, STANDUP);
}

#[test]
fn a_path_under_the_link_itself_still_counts() {
    let root = LinkedRoot::new("as-given");
    let mut index = Index::open(&root.link).unwrap();
    let notes = root.link.join(STANDUP).join(NOTES_FILE);
    write_later(&notes, "Quokka budget.\n");

    assert_eq!(index.update(&root.link, &[notes]).unwrap(), 1);
    assert_eq!(index.search("quokka").unwrap().len(), 1);
}

#[test]
fn catch_up_works_through_the_link() {
    let root = LinkedRoot::new("catch-up");
    drop(Index::open(&root.link).unwrap());
    write_later(
        &root.real.join(STANDUP).join(NOTES_FILE),
        "Kangaroo review.\n",
    );

    let mut index = Index::open(&root.link).unwrap();
    assert_eq!(index.catch_up(&root.link).unwrap(), 1);
    assert_eq!(index.search("kangaroo").unwrap().len(), 1);
}
