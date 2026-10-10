//! Where the meetings root is, and moving it.
//!
//! The one thing `store` deliberately does not know about: the root pointer
//! file in the OS config folder, the environment override, and the move that
//! carries every meeting to a new folder (`move_tree`, TUR-149).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::list::{Live, MeetingList, list};
use crate::error::UiError;

mod move_tree;
mod paths;

/// The kind of error for a pointer file that is there but will not read. The
/// webview keeps the user out of onboarding on it (TUR-149): whether setup was
/// done is recorded inside the very folder this file names.
pub(super) const POINTER_UNREADABLE: &str = "root-pointer-unreadable";

/// Where meetings live.
///
/// `~/Meetings` per SPEC §3.1, built with `dirs` + `PathBuf::join` and no
/// literal `~` (the Windows seam, SPEC §8.2) — unless the user has picked
/// somewhere else, in which case [`configured_root`] wins.
///
/// `MEET_AI_MEETINGS_ROOT` overrides both. That exists so this screen can be
/// driven against a fixture folder without writing into the developer's real
/// meetings. It is read under the same rule as `stt::model::default_model_dir`
/// (one variable name, and empty means unset rather than "the current
/// directory"), so the app and the CLI tools cannot resolve two different roots
/// from one environment.
pub fn root() -> Result<PathBuf, UiError> {
    if let Some(custom) =
        std::env::var_os(stt::model::MEETINGS_ROOT_ENV).filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(custom));
    }
    if let Some(configured) = configured_root()? {
        return Ok(configured);
    }
    dirs::home_dir()
        .map(|home| meeting_format::layout::default_root(&home))
        .ok_or_else(|| {
            UiError::app(
                "no-home-dir",
                "meet-ai could not work out where your home folder is, so it does not know where \
                 to keep your meetings.",
            )
        })
}

/// Where the app remembers a user-chosen meetings folder.
///
/// This cannot live inside the meetings root itself — the whole point of the
/// pointer is to find the root before reading anything under it, and a folder
/// that has just been moved away from is the one place we can no longer read.
/// It lives in the OS's own per-app support folder instead: one small file
/// with nothing meeting-shaped in it, so this does not conflict with L10 (the
/// app writes nothing *meeting* data outside `~/Meetings/`).
fn pointer_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("meet-ai").join("root.json"))
}

/// `root.json`. The path is a `PathBuf`, so it is written exactly or not at
/// all: serde refuses a path that is not valid UTF-8 instead of saving a
/// lossy copy of it that names some other folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RootPointer {
    pub(super) custom_root: Option<PathBuf>,
}

/// The user's chosen folder, if they ever changed it from the default.
///
/// No pointer file means no override, and the app uses `~/Meetings`. A
/// pointer that is there but will not read is an error, never a quiet fall
/// back to `~/Meetings`: that would show an empty meetings list and the setup
/// screens as if every meeting were gone (TUR-149).
fn configured_root() -> Result<Option<PathBuf>, UiError> {
    match pointer_path() {
        Some(path) => read_pointer(&path),
        None => Ok(None),
    }
}

/// Logged once per launch: `root()` is asked on every command.
static POINTER_ERROR_LOGGED: AtomicBool = AtomicBool::new(false);

pub(super) fn read_pointer(path: &Path) -> Result<Option<PathBuf>, UiError> {
    let parsed = match fs::read(path) {
        Ok(raw) => serde_json::from_slice::<RootPointer>(&raw).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => Err(error.to_string()),
    };
    match parsed {
        Ok(pointer) => Ok(pointer
            .custom_root
            .filter(|root| !root.as_os_str().is_empty())),
        Err(error) => {
            if !POINTER_ERROR_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::error!(%error, path = %path.display(), "the meetings folder pointer will not read");
            }
            Err(UiError::app(
                POINTER_UNREADABLE,
                format!(
                    "The file that remembers where your meetings folder is ({}) is damaged: \
                     {error}. Your meetings were not touched. Pick your meetings folder again \
                     in Settings.",
                    path.display()
                ),
            ))
        }
    }
}

/// The pointer's contents for `new_root`, worked out before anything moves so
/// a path that cannot be saved stops the move at the start, not the end.
fn pointer_body(new_root: &Path) -> Result<Vec<u8>, UiError> {
    let pointer = RootPointer {
        custom_root: Some(new_root.to_path_buf()),
    };
    serde_json::to_vec_pretty(&pointer).map_err(|error| {
        UiError::app(
            "unsupported-folder-name",
            format!(
                "meet-ai cannot remember \"{}\" as your meetings folder ({error}). Pick a folder \
                 whose name has only ordinary letters.",
                new_root.display()
            ),
        )
    })
}

/// Save the pointer through `write_atomic`: a crash or a full disk leaves the
/// old pointer or the new one, never half of one.
pub(super) fn write_pointer_at(pointer: &Path, body: &[u8]) -> Result<(), UiError> {
    if let Some(parent) = pointer.parent() {
        fs::create_dir_all(parent)?;
    }
    meeting_format::write_atomic(pointer, body)?;
    Ok(())
}

/// Move the meetings folder to `new_root`, taking every existing meeting with
/// it, and remember the new location for next launch.
///
/// Existing files always move — they are never left behind at the old path.
/// A meetings list that quietly stopped showing yesterday's standup the moment
/// someone picked a new folder would look exactly like data loss, even though
/// nothing was actually deleted. A move that fails at any step leaves every
/// meeting in the old folder, and the app pointing at it (see `move_tree`).
///
/// With a damaged pointer there is no current folder to move from, so picking
/// a folder only points the app at it: the way out of that error.
pub fn change_root(new_root: PathBuf) -> Result<MeetingList, UiError> {
    let pointer = pointer_path().ok_or_else(|| {
        UiError::app(
            "no-config-dir",
            "meet-ai could not find a place on this computer to remember your chosen folder.",
        )
    })?;
    let old_root = match root() {
        Ok(old_root) => Some(old_root),
        Err(error) if error.kind == POINTER_UNREADABLE => None,
        Err(error) => return Err(error),
    };

    let new_root = match old_root {
        Some(old_root) => {
            let (old_root, new_root) = paths::checked(&old_root, &new_root)?;
            let body = pointer_body(&new_root)?;
            move_tree::move_root(&old_root, &new_root, &move_tree::RealFs, &|_| {
                write_pointer_at(&pointer, &body)
            })?;
            new_root
        }
        None => {
            let new_root = paths::checked_alone(&new_root)?;
            let body = pointer_body(&new_root)?;
            tracing::warn!(new = %new_root.display(), "replacing a damaged meetings folder pointer");
            move_tree::point_at(&new_root, &|_| write_pointer_at(&pointer, &body))?;
            new_root
        }
    };

    // Crash files follow the folder (TUR-149).
    crate::logs::follow_root(&new_root);
    // The IPC command refuses a move unless the recorder is idle, so nothing
    // under the new root is being written.
    list(Live::Nothing)
}

#[cfg(test)]
mod tests;
