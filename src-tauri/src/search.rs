//! Search across meetings (TUR-101).
//!
//! The index itself is [`store::index`]: a derived SQLite file that is rebuilt
//! from the markdown whenever it is missing (L7). This file owns the one open
//! copy, keeps it pointed at the current meetings folder, and answers the
//! window's `search` command.
//!
//! The index is opened lazily, on the first search or the first folder change
//! the watcher reports, and also once at launch ([`SearchIndex::warm`]) so the
//! first search is not the one that pays for a full rescan.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use store::index::{Hit, Index, IndexedMeeting};
use tauri::{AppHandle, Manager as _};

use crate::error::UiError;
use crate::meetings;

/// The open index and the meetings folder it was built for.
#[derive(Default)]
pub struct SearchIndex {
    open: Mutex<Option<(PathBuf, Index)>>,
}

impl SearchIndex {
    /// Open (and, if the file is missing or old, build) the index for the
    /// current meetings folder. Safe to call any time; does nothing when the
    /// folder does not exist yet.
    pub fn warm(&self) {
        if let Err(error) = self.with_index(|_, _| Ok(())) {
            tracing::warn!(message = %error.message, "could not prepare the search index");
        }
    }

    /// Re-read the meetings behind `paths` (the watcher's changed files).
    pub fn update(&self, paths: &[PathBuf]) {
        let result = self.with_index(|root, index| {
            index
                .update(root, paths)
                .map(|_| ())
                .map_err(|error| UiError::app("search-index", error.to_string()))
        });
        if let Err(error) = result {
            tracing::warn!(message = %error.message, "could not update the search index");
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

/// The app's [`SearchIndex`], from managed state.
pub fn state(app: &AppHandle) -> tauri::State<'_, SearchIndex> {
    app.state::<SearchIndex>()
}

/// Search every meeting's transcript, notes, summary and tickets.
///
/// Runs on the blocking pool: the first call after launch can still be
/// rebuilding the index, and SQLite is disk work.
#[tauri::command]
#[specta::specta]
pub async fn search(app: AppHandle, query: String) -> Result<Vec<Hit>, UiError> {
    tauri::async_runtime::spawn_blocking(move || state(&app).search(&query))
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))?
}
