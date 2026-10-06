//! Tells the window when the meetings folder changes outside the app (TUR-100).
//!
//! An agent or the user editing `meeting.md` in another editor should show up
//! in the list without a restart. The work of noticing is in
//! [`store::watcher`]; this file owns the one running watcher, points it at the
//! current meetings folder, and turns each batch of changes into a single
//! [`crate::events::MEETINGS_CHANGED_EVENT`].
//!
//! The app's own writes are the trap. Saving a note writes `notes.md`, which
//! the watcher would report, and the window would reload under the user's
//! cursor. [`MeetingsWatch::note_own_write`] records those writes so the
//! watcher skips them (SPEC §4).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use store::watcher::{SelfWrites, Watcher};
use tauri::{AppHandle, Emitter as _, Manager as _};

use crate::{meetings, search};

/// What the window receives: the files that changed. The list refreshes on any
/// change; it never moves the selection.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Changed {
    pub paths: Vec<String>,
}

impl Changed {
    fn new(paths: &[PathBuf]) -> Self {
        Self {
            paths: paths.iter().map(|p| p.display().to_string()).collect(),
        }
    }
}

/// How the last attempt to watch the meetings folder went.
enum Attempt {
    /// Watching; dropping the watcher stops it.
    Running(Watcher),
    /// The folder did not exist yet (first launch, before the first recording).
    Missing,
    /// The folder exists but could not be watched; not retried on its own.
    Failed(String),
}

/// What [`MeetingsWatch::ensure_running`] does next.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Nothing,
    Start,
}

/// Start a watcher only when the last attempt found the folder missing and it
/// exists now. A running watcher is left alone, and a failure is not retried
/// on every list refresh.
fn next(state: &Attempt, root_is_dir: bool) -> Action {
    match state {
        Attempt::Missing if root_is_dir => Action::Start,
        Attempt::Missing | Attempt::Running(_) | Attempt::Failed(_) => Action::Nothing,
    }
}

/// The running watcher, if any, plus the record of our own writes.
pub struct MeetingsWatch {
    running: Mutex<Attempt>,
    own_writes: SelfWrites,
}

impl Default for MeetingsWatch {
    fn default() -> Self {
        Self {
            running: Mutex::new(Attempt::Missing),
            own_writes: SelfWrites::default(),
        }
    }
}

impl MeetingsWatch {
    /// Record that this process just wrote `path`, so the watcher ignores the
    /// echo. Call it right after the write finishes.
    pub fn note_own_write(&self, path: &Path) {
        self.own_writes.note(path);
    }

    /// The record of this process's own writes, for code that writes several
    /// files through `store` (the notes run, TUR-10).
    pub fn own_writes(&self) -> &SelfWrites {
        &self.own_writes
    }

    /// Watch the current meetings folder, replacing any watcher already
    /// running. Called at launch and after the folder moves.
    ///
    /// A folder that does not exist yet (first launch, before the first
    /// recording) is not an error: there is nothing to watch, so the old
    /// watcher is dropped and the attempt is recorded as missing for
    /// [`Self::ensure_running`] to pick up.
    pub fn restart(&self, app: &AppHandle) {
        let mut guard = self.lock();
        // Drop the old watcher first, so two never run against one folder.
        *guard = Attempt::Missing;
        self.start_into(app, &mut guard);
    }

    /// Start watching if the folder was missing at the last attempt and has
    /// appeared since. Cheap when nothing changed; called from
    /// `list_meetings`, which the window runs after a recording starts or
    /// stops. The check and the start happen under one lock, so two callers
    /// never start two watchers.
    pub fn ensure_running(&self, app: &AppHandle) {
        let mut guard = self.lock();
        let is_dir = meetings::root().is_ok_and(|root| root.is_dir());
        if next(&guard, is_dir) == Action::Start {
            self.start_into(app, &mut guard);
        }
    }

    fn start_into(&self, app: &AppHandle, slot: &mut Attempt) {
        let root = match meetings::root() {
            Ok(root) => root,
            Err(error) => {
                tracing::warn!(message = %error.message, "no meetings folder to watch");
                *slot = Attempt::Failed(error.message);
                return;
            }
        };
        if !root.is_dir() {
            *slot = Attempt::Missing;
            return;
        }
        let handle = app.clone();
        let started = Watcher::start(&root, self.own_writes.clone(), move |paths| {
            // TUR-101: keep the search index current before the window refreshes.
            search::state(&handle).update(&paths);
            if let Err(error) =
                handle.emit(crate::events::MEETINGS_CHANGED_EVENT, Changed::new(&paths))
            {
                tracing::warn!(%error, "could not tell the window the meetings folder changed");
            }
        });
        *slot = match started {
            Ok(watcher) => Attempt::Running(watcher),
            Err(error) => {
                tracing::warn!(%error, "could not watch the meetings folder");
                Attempt::Failed(error.to_string())
            }
        };
    }

    fn lock(&self) -> MutexGuard<'_, Attempt> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The app's [`MeetingsWatch`], from managed state.
pub fn state(app: &AppHandle) -> tauri::State<'_, MeetingsWatch> {
    app.state::<MeetingsWatch>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_that_appeared_starts_a_watcher() {
        assert_eq!(next(&Attempt::Missing, true), Action::Start);
    }

    #[test]
    fn a_missing_folder_that_is_still_missing_waits() {
        assert_eq!(next(&Attempt::Missing, false), Action::Nothing);
    }

    #[test]
    fn a_failed_attempt_is_not_retried() {
        assert_eq!(next(&Attempt::Failed("x".into()), true), Action::Nothing);
    }

    #[test]
    fn a_running_watcher_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let watcher = Watcher::start(dir.path(), SelfWrites::default(), |_| {}).expect("watch");
        assert_eq!(next(&Attempt::Running(watcher), true), Action::Nothing);
    }

    #[test]
    fn the_payload_lists_each_path_as_text() {
        let changed = Changed::new(&[PathBuf::from("/m/a/notes.md")]);
        assert_eq!(changed.paths, vec!["/m/a/notes.md".to_string()]);
    }
}
