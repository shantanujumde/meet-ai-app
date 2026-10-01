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

/// The running watcher, if any, plus the record of our own writes.
#[derive(Default)]
pub struct MeetingsWatch {
    running: Mutex<Option<Watcher>>,
    own_writes: SelfWrites,
}

impl MeetingsWatch {
    /// Record that this process just wrote `path`, so the watcher ignores the
    /// echo. Call it right after the write finishes.
    pub fn note_own_write(&self, path: &Path) {
        self.own_writes.note(path);
    }

    /// Watch the current meetings folder, replacing any watcher already
    /// running. Called at launch and after the folder moves.
    ///
    /// A folder that does not exist yet (first launch, before the first
    /// recording) is not an error: there is nothing to watch, so the old
    /// watcher is dropped and this returns.
    pub fn restart(&self, app: &AppHandle) {
        // Drop the old watcher first, so two never run against one folder.
        self.stop();
        let root = match meetings::root() {
            Ok(root) => root,
            Err(error) => {
                tracing::warn!(message = %error.message, "no meetings folder to watch");
                return;
            }
        };
        if !root.is_dir() {
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
        match started {
            Ok(watcher) => *self.lock() = Some(watcher),
            Err(error) => tracing::warn!(%error, "could not watch the meetings folder"),
        }
    }

    fn stop(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> MutexGuard<'_, Option<Watcher>> {
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
    fn the_payload_lists_each_path_as_text() {
        let changed = Changed::new(&[PathBuf::from("/m/a/notes.md")]);
        assert_eq!(changed.paths, vec!["/m/a/notes.md".to_string()]);
    }
}
