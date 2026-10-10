//! Search across meetings (TUR-101).
//!
//! The index itself is [`store::index`]: a derived SQLite file that is rebuilt
//! from the markdown whenever it is missing (L7). This file owns the one open
//! copy, keeps it pointed at the current meetings folder, and answers the
//! window's `search` command.
//!
//! The index is opened lazily, on the first search or the first folder change
//! the watcher reports, and also once at launch ([`SearchIndex::warm`]) so the
//! first search is not the one that pays for a full rescan. That launch call
//! also catches a kept index up with edits made while the app was closed
//! (TUR-152).
//!
//! The watcher skips the app's own writes, so code that changes a meeting's
//! files itself calls [`meeting_written`] (TUR-107, TUR-152): the agent's
//! notes, title and tickets, the calendar's title, a rename and the user's
//! `notes.md`. Otherwise search, and the pre-meeting brief, which finds past
//! meetings by title, would not see them until the next rescan.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use store::index::{Hit, Index, IndexedMeeting};
use tauri::{AppHandle, Manager as _};

use crate::error::{UiError, on_blocking_pool};
use crate::meetings::{self, Live};
use crate::recording::Recorder;

/// The open index and the meetings folder it was built for.
#[derive(Default)]
pub struct SearchIndex {
    open: Mutex<Option<(PathBuf, Index)>>,
}

impl SearchIndex {
    /// Open (and, if the file is missing or old, build) the index for the
    /// current meetings folder, then catch it up with edits made while the
    /// app was closed (TUR-152). Safe to call any time; does nothing when the
    /// folder does not exist yet. Reads the disk, so call it off the UI
    /// thread (launch runs it on its own thread).
    pub fn warm(&self) {
        let result = self.with_index(|root, index| {
            let changed = index
                .catch_up(root)
                .map_err(|error| UiError::app("search-index", error.to_string()))?;
            tracing::info!(changed, "search index caught up with the meetings folder");
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!(message = %error.message, "could not prepare the search index");
        }
    }

    /// Re-read the meetings behind `paths` (the watcher's changed files).
    /// `live` is the meeting being recorded, re-read at most every
    /// [`store::index::LIVE_REINDEX_EVERY`] (TUR-166).
    pub fn update(&self, paths: &[PathBuf], live: Live<'_>) {
        let result = self.with_index(|root, index| {
            index
                .set_live(root, live_id(live))
                .and_then(|_| index.update(root, paths))
                .map(|_| ())
                .map_err(|error| UiError::app("search-index", error.to_string()))
        });
        if let Err(error) = result {
            tracing::warn!(message = %error.message, "could not update the search index");
        }
    }

    /// Tell the index which meeting is recording. When one stops, its
    /// changes held back while it recorded are indexed now (TUR-166).
    pub fn set_live(&self, live: Live<'_>) {
        let result = self.with_index(|root, index| {
            index
                .set_live(root, live_id(live))
                .map(|_| ())
                .map_err(|error| UiError::app("search-index", error.to_string()))
        });
        if let Err(error) = result {
            tracing::warn!(message = %error.message, "could not update the search index");
        }
    }

    /// Re-read one meeting that the app just wrote, whatever the watcher
    /// makes of it. See [`meeting_written`].
    pub fn refresh_meeting(&self, meeting_id: &str) {
        let result = self.with_index(|root, index| {
            index
                .refresh_meeting(root, meeting_id)
                .map_err(|error| UiError::app("search-index", error.to_string()))
        });
        if let Err(error) = result {
            tracing::warn!(meeting_id, message = %error.message, "could not update the search index");
        }
    }

    /// Find `query` in every meeting. A folder that does not exist has no
    /// meetings, so no hits.
    pub fn search(&self, query: &str) -> Result<Vec<Hit>, UiError> {
        let mut hits = Vec::new();
        self.with_index(|_, index| {
            hits = index
                .search(query)
                .map_err(|error| UiError::app("search-index", error.to_string()))?;
            Ok(())
        })?;
        Ok(hits)
    }

    /// Every meeting titled `title` (any case, trimmed), newest first, for the
    /// pre-meeting brief (TUR-32). No meetings folder yet means none.
    pub fn meetings_titled(&self, title: &str) -> Result<Vec<IndexedMeeting>, UiError> {
        let mut found = Vec::new();
        self.with_index(|_, index| {
            found = index
                .meetings_titled(title)
                .map_err(|error| UiError::app("search-index", error.to_string()))?;
            Ok(())
        })?;
        Ok(found)
    }

    /// Run `work` on the index for the current folder, opening it first when
    /// there is none or the folder changed. Skips `work` when there is no
    /// meetings folder yet.
    fn with_index(
        &self,
        work: impl FnOnce(&Path, &mut Index) -> Result<(), UiError>,
    ) -> Result<(), UiError> {
        let root = meetings::root()?;
        if !root.is_dir() {
            return Ok(());
        }
        let mut open = self.lock();
        if open.as_ref().is_none_or(|(opened, _)| *opened != root) {
            let index = Index::open(&root)
                .map_err(|error| UiError::app("search-index", error.to_string()))?;
            *open = Some((root.clone(), index));
        }
        match open.as_mut() {
            Some((_, index)) => work(&root, index),
            None => Ok(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<(PathBuf, Index)>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn live_id<'a>(live: Live<'a>) -> Option<&'a str> {
    match live {
        Live::Nothing => None,
        Live::Meeting(id) => Some(id),
    }
}

/// The watcher saw `paths` change: bring the index up to date, holding back
/// the meeting being recorded (TUR-166).
pub fn folder_changed(app: &AppHandle, paths: &[PathBuf]) {
    let status = app.state::<Recorder>().status();
    state(app).update(paths, Live::from_status(&status));
}

/// The app's [`SearchIndex`], from managed state.
pub fn state(app: &AppHandle) -> tauri::State<'_, SearchIndex> {
    app.state::<SearchIndex>()
}

/// The app just wrote meeting `meeting_id`'s `meeting.md` (TUR-107): bring its
/// row in the index up to date now. Call it after the write, outside any lock
/// on the meeting. Does nothing when the index is not managed (unit tests).
pub fn meeting_written(app: &AppHandle, meeting_id: &str) {
    if let Some(index) = app.try_state::<SearchIndex>() {
        index.refresh_meeting(meeting_id);
    }
}

/// Search every meeting's transcript, notes, summary and tickets.
///
/// Runs on the blocking pool: the first call after launch can still be
/// rebuilding the index, and SQLite is disk work.
#[tauri::command]
#[specta::specta]
pub async fn search(app: AppHandle, query: String) -> Result<Vec<Hit>, UiError> {
    on_blocking_pool(move || state(&app).search(&query)).await?
}
