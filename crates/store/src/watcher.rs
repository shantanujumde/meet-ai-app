//! Watching the meetings folder for changes made by someone else.
//!
//! Phase 3b (SPEC §4, TUR-100). An agent, the user's editor or a sync tool can
//! change any file under the root while the app is open. [`Watcher`] turns that
//! into one callback per burst of changes, carrying the paths that changed.
//!
//! Three kinds of noise are filtered out before the callback runs:
//!
//! * **Dotfiles and dot-folders.** Atomic writes go through a dotfile temp
//!   (see [`crate::write_atomic`]) and `.git` churns on its own. Neither is
//!   content. Any path with a component starting with `.` *below the root* is
//!   dropped; a dot in the root's own path (say `~/.meetings`) does not count.
//! * **`index.db` and its sidecars.** The index is derived (L7). Reacting to
//!   our own index update would loop forever.
//! * **This process's own writes.** See [`SelfWrites`], including the notes the
//!   app writes from the agent's JSON. Files an agent writes itself (the
//!   copy-prompt path) are not suppressed.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use notify_debouncer_full::notify::{self, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::{SELF_WRITE_SUPPRESSION, WATCH_DEBOUNCE};

/// The file name prefix of the derived index and its SQLite sidecars
/// (`index.db-wal`, `index.db-shm`, `index.db-journal`).
const INDEX_DB_PREFIX: &str = "index.db";

/// Paths this process wrote recently, so the watcher can ignore their echo.
///
/// The app writes `notes.md` while the user types. Without this, the watcher
/// would report that write, the app would reload the file and the cursor would
/// jump mid-sentence. Call [`SelfWrites::note`] right after each write the app
/// makes itself. Cheap to clone: every clone shares the same record.
#[derive(Clone, Default)]
pub struct SelfWrites {
    noted: Arc<Mutex<HashMap<PathBuf, Instant>>>,
}

impl SelfWrites {
    /// Record that this process just wrote `path`.
    ///
    /// The path is stored canonicalised, because the watcher reports real
    /// paths (on macOS `/var` shows up as `/private/var`). If the path cannot
    /// be canonicalised, it is stored as given. Entries that have outlived the
    /// suppression window are dropped here, so the map never grows without
    /// bound.
    pub fn note(&self, path: &Path) {
        let key = stored_form(path);
        let now = Instant::now();
        let mut noted = lock(&self.noted);
        noted.retain(|_, at| now.saturating_duration_since(*at) < SELF_WRITE_SUPPRESSION);
        noted.insert(key, now);
    }

    /// Was `path` noted less than [`SELF_WRITE_SUPPRESSION`] before `now`?
    ///
    /// `path` is compared in its stored form, so pass the same kind of path
    /// that was given to [`SelfWrites::note`] (or its canonical form). The
    /// watcher passes the time the event was seen as `now`, not the time it
    /// handles the debounced burst. `now` is a parameter so tests can move
    /// time without sleeping.
    pub fn is_suppressed(&self, path: &Path, now: Instant) -> bool {
        lock(&self.noted).get(path).is_some_and(|at| {
            // An entry noted after `now` counts as fresh, not as an underflow.
            now.saturating_duration_since(*at) < SELF_WRITE_SUPPRESSION
        })
    }
}

/// Canonical form of `path`, or `path` itself when it does not exist (yet).
fn stored_form(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Lock the map, carrying on if another thread panicked while holding it. The
/// map is only ever a set of timestamps, so a half-finished update is harmless.
fn lock(map: &Mutex<HashMap<PathBuf, Instant>>) -> MutexGuard<'_, HashMap<PathBuf, Instant>> {
    map.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A running watch on the meetings folder. Dropping it stops watching.
pub struct Watcher {
    // Held only for its `Drop`, which stops the watcher thread.
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

impl Watcher {
    /// Start watching `root` and everything under it.
    ///
    /// `on_change` runs on the watcher's own thread, at most once per
    /// [`WATCH_DEBOUNCE`] burst, with a sorted, de-duplicated list of changed
    /// paths. It is never called with an empty list. Keep it quick: hand the
    /// paths to the app and return.
    ///
    /// Paths are reported as the OS reports them, which is the canonical form
    /// of `root` plus the path below it.
    pub fn start(
        root: &Path,
        self_writes: SelfWrites,
        on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
    ) -> Result<Watcher, crate::Error> {
        let root = std::fs::canonicalize(root)?;
        let filter_root = root.clone();

        let mut debouncer = new_debouncer(
            WATCH_DEBOUNCE,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    // Each path carries the time its event reached the
                    // debouncer, not the time this burst is handed over. The
                    // debouncer holds an event for WATCH_DEBOUNCE plus up to a
                    // tick before calling us; measuring the suppression window
                    // from here would leave a busy Mac about 125 ms of slack,
                    // and our own write would be reported (TUR-83).
                    let paths = events
                        .iter()
                        .filter(|event| !matches!(event.kind, EventKind::Access(_)))
                        .flat_map(|event| {
                            event.paths.iter().map(|path| (path.clone(), event.time))
                        });
                    let changed = filter_paths(paths, &filter_root, &self_writes);
                    if !changed.is_empty() {
                        on_change(changed);
                    }
                }
                Err(errors) => {
                    for error in errors {
                        tracing::warn!(%error, "file watcher error");
                    }
                }
            },
        )
        .map_err(to_io)?;

        debouncer
            .watch(&root, RecursiveMode::Recursive)
            .map_err(to_io)?;

        Ok(Watcher {
            _debouncer: debouncer,
        })
    }
}

fn to_io(error: notify::Error) -> crate::Error {
    crate::Error::Io(std::io::Error::other(error))
}

/// Apply every filter to a batch of event paths: noise out, own writes out,
/// then sorted and de-duplicated.
///
/// Each path comes with the moment its event was seen, and suppression is
/// judged at that moment (SPEC §4: drop events matching an entry newer than
/// 750 ms), so the debounce delay does not eat into the window.
fn filter_paths(
    paths: impl IntoIterator<Item = (PathBuf, Instant)>,
    root: &Path,
    self_writes: &SelfWrites,
) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = paths
        .into_iter()
        .filter(|(path, _)| !is_noise(path, root))
        .filter(|(path, seen)| !self_writes.is_suppressed(path, *seen))
        .map(|(path, _)| path)
        .collect();
    kept.sort();
    kept.dedup();
    kept
}

/// Is `path` a hidden file or folder below `root`, or part of the derived
/// index?
fn is_noise(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let hidden = relative.components().any(|component| match component {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    });
    let index = path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with(INDEX_DB_PREFIX));
    hidden || index
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn self_write_pause_outlasts_debounce() {
        // If this ever inverts, a self-write echo arrives after suppression
        // has already expired and the user's cursor jumps.
        assert!(SELF_WRITE_SUPPRESSION > WATCH_DEBOUNCE);
    }

    #[test]
    fn self_writes_suppress_only_the_noted_path_and_only_for_the_window() {
        let _dir_guard = scratch_dir("selfwrites-unit");
        let dir = _dir_guard.path().to_path_buf();
        let noted = dir.join("notes.md");
        let other = dir.join("meeting.md");
        std::fs::write(&noted, "x").unwrap();

        let writes = SelfWrites::default();
        let before = Instant::now();
        writes.note(&noted);
        let stored = stored_form(&noted);

        assert!(writes.is_suppressed(&stored, Instant::now()));
        assert!(!writes.is_suppressed(&other, Instant::now()));
        let later = before + SELF_WRITE_SUPPRESSION + Duration::from_millis(1);
        // `note` ran after `before`, so this is past the window for certain.
        let later = later + Duration::from_millis(50);
        assert!(!writes.is_suppressed(&stored, later));
    }

    #[test]
    fn note_prunes_expired_entries() {
        let writes = SelfWrites::default();
        let stale = PathBuf::from("/nowhere/stale.md");
        lock(&writes.noted).insert(
            stale.clone(),
            Instant::now() - SELF_WRITE_SUPPRESSION - Duration::from_millis(10),
        );
        writes.note(Path::new("/nowhere/fresh.md"));
        assert!(!lock(&writes.noted).contains_key(&stale));
    }

    #[test]
    fn hidden_and_index_paths_are_noise() {
        let root = Path::new("/home/me/.meetings");
        assert!(is_noise(&root.join(".git/HEAD"), root));
        assert!(is_noise(&root.join("m1/.notes.md.tmp"), root));
        assert!(is_noise(&root.join("index.db"), root));
        assert!(is_noise(&root.join("index.db-wal"), root));
        assert!(!is_noise(&root.join("m1/notes.md"), root));
    }

    #[test]
    fn suppression_is_judged_when_the_event_was_seen_not_when_the_burst_is_handed_over() {
        // TUR-83. The debouncer hands a burst over WATCH_DEBOUNCE plus up to a
        // tick after the event arrived; a busy runner adds more. Measured from
        // hand-over, that ate the window and our own write was reported.
        let _dir_guard = scratch_dir("selfwrites-seen");
        let root = std::fs::canonicalize(_dir_guard.path()).unwrap();
        let file = root.join("notes.md");
        let other = root.join("meeting.md");
        std::fs::write(&file, "x").unwrap();

        let writes = SelfWrites::default();
        writes.note(&file);
        let noted = lock(&writes.noted)[&file];
        // Seen 50 ms after the note. `filter_paths` reads no clock, so however
        // late the burst is handed over, only this time counts.
        let seen = noted + Duration::from_millis(50);

        let kept = filter_paths(
            [(file.clone(), seen), (other.clone(), seen)],
            &root,
            &writes,
        );
        assert_eq!(kept, vec![other]);

        // An edit seen after the window is someone else's and is reported.
        let late = noted + SELF_WRITE_SUPPRESSION + Duration::from_millis(1);
        assert_eq!(
            filter_paths([(file.clone(), late)], &root, &writes),
            vec![file]
        );
    }

    #[test]
    fn an_external_write_is_reported() {
        let _root_guard = scratch_dir("external");
        let root = _root_guard.path().to_path_buf();
        let folder = root.join("2026-09-01-1430-standup");
        std::fs::create_dir_all(&folder).unwrap();

        let (tx, rx) = mpsc::channel();
        let _watcher = Watcher::start(&root, SelfWrites::default(), move |paths| {
            tx.send(paths).ok();
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));

        let file = folder.join("notes.md");
        std::fs::write(&file, "from an agent").unwrap();
        let expected = std::fs::canonicalize(&file).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = false;
        while !seen && Instant::now() < deadline {
            if let Ok(paths) = rx.recv_timeout(Duration::from_millis(250)) {
                seen = paths.contains(&expected);
            }
        }
        assert!(seen, "no change reported for {expected:?}");
    }

    #[test]
    fn a_noted_self_write_is_not_reported() {
        let _root_guard = scratch_dir("selfwrite");
        let root = _root_guard.path().to_path_buf();
        let folder = root.join("2026-09-01-1430-standup");
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("notes.md");
        // Exists before it is noted, so `note` stores the canonical path.
        std::fs::write(&file, "start").unwrap();

        let writes = SelfWrites::default();
        let (tx, rx) = mpsc::channel();
        let _watcher = Watcher::start(&root, writes.clone(), move |paths| {
            tx.send(paths).ok();
        })
        .unwrap();

        // FSEvents can deliver the setup write late. Drain until the watcher
        // has been quiet for a full debounce plus a margin, so that noise
        // cannot be mistaken for the self-write.
        let quiet = WATCH_DEBOUNCE + Duration::from_millis(500);
        while rx.recv_timeout(quiet).is_ok() {}

        let expected = std::fs::canonicalize(&file).unwrap();
        writes.note(&file);
        std::fs::write(&file, "typed by the user").unwrap();
        // An un-noted write beside it, in the same burst. It must be reported,
        // which proves the watcher was delivering at all: with nothing
        // delivered, "the self-write never arrived" would pass on its own.
        let sibling = folder.join("meeting.md");
        std::fs::write(&sibling, "edited by an agent").unwrap();
        let sibling = std::fs::canonicalize(&sibling).unwrap();

        let mut sibling_seen = false;
        let deadline = Instant::now() + WATCH_DEBOUNCE + Duration::from_secs(5);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            // Once the sibling is in, wait one more quiet debounce for a late
            // copy of the self-write, then stop.
            let wait = if sibling_seen { left.min(quiet) } else { left };
            match rx.recv_timeout(wait) {
                Ok(paths) => {
                    assert!(
                        !paths.contains(&expected),
                        "a self-write must not reach on_change: {paths:?}"
                    );
                    sibling_seen |= paths.contains(&sibling);
                }
                Err(_) if sibling_seen => break,
                Err(_) => {}
            }
        }
        assert!(
            sibling_seen,
            "the un-noted write beside it was never reported"
        );
    }

    fn scratch_dir(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-watcher-{name}-"))
            .tempdir()
            .unwrap()
    }
}
