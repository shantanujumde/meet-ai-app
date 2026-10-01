//! Where the meetings root is, and moving it.
//!
//! The one thing `store` deliberately does not know about: the root pointer
//! file in the OS config folder, the environment override, and the move that
//! carries every meeting to a new folder.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::list::{Live, MeetingList, list};
use crate::error::UiError;

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
    if let Some(configured) = configured_root() {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RootPointer {
    pub(super) custom_root: Option<String>,
}

/// The user's chosen folder, if they ever changed it from the default.
///
/// Same failure rule as everywhere else in this module: a missing or corrupt
/// pointer is not an error, it just means "no override", and the app falls
/// back to `~/Meetings` rather than refusing to start.
fn configured_root() -> Option<PathBuf> {
    let raw = fs::read_to_string(pointer_path()?).ok()?;
    let pointer: RootPointer = serde_json::from_str(&raw).ok()?;
    pointer
        .custom_root
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn write_pointer(new_root: &Path) -> Result<(), UiError> {
    let path = pointer_path().ok_or_else(|| {
        UiError::app(
            "no-config-dir",
            "meet-ai could not find a place on this Mac to remember your chosen folder.",
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pointer = RootPointer {
        custom_root: Some(new_root.display().to_string()),
    };
    let body = serde_json::to_string_pretty(&pointer)
        .map_err(|error| UiError::app("serialize", error.to_string()))?;
    fs::write(&path, body)?;
    Ok(())
}

/// Move the meetings folder to `new_root`, taking every existing meeting with
/// it, and remember the new location for next launch.
///
/// Existing files always move — they are never left behind at the old path.
/// A meetings list that quietly stopped showing yesterday's standup the moment
/// someone picked a new folder would look exactly like data loss, even though
/// nothing was actually deleted.
pub fn change_root(new_root: PathBuf) -> Result<MeetingList, UiError> {
    let old_root = root()?;

    if new_root == old_root {
        return Err(UiError::app(
            "same-folder",
            "That is already your meetings folder.",
        ));
    }
    if new_root.starts_with(&old_root) || old_root.starts_with(&new_root) {
        return Err(UiError::app(
            "nested-folder",
            "The new folder can't be inside your current meetings folder, or the other way \
             around.",
        ));
    }

    if old_root.is_dir() {
        move_contents(&old_root, &new_root)?;
    } else {
        fs::create_dir_all(&new_root)?;
    }

    write_pointer(&new_root)?;
    // The IPC command refuses a move unless the recorder is idle, so nothing
    // under the new root is being written.
    list(Live::Nothing)
}

/// Move everything from `old_root` into `new_root`, merging rather than
/// clobbering if `new_root` already exists (e.g. the user picked an existing
/// folder inside an already-synced Dropbox or iCloud Drive location).
pub(super) fn move_contents(old_root: &Path, new_root: &Path) -> Result<(), UiError> {
    if !new_root.exists() {
        if let Some(parent) = new_root.parent() {
            fs::create_dir_all(parent)?;
        }
        // The common case: one atomic rename, nothing to merge.
        if fs::rename(old_root, new_root).is_ok() {
            return Ok(());
        }
        // `rename(2)` refuses to jump filesystems (e.g. onto a different
        // volume), so fall back to an explicit copy-then-delete.
        copy_dir(old_root, new_root)?;
        fs::remove_dir_all(old_root)?;
        return Ok(());
    }

    // The destination already has something in it. Refuse outright on any
    // name collision rather than guessing which of two same-named folders is
    // the real meeting — silently overwriting one would be a straightforward
    // way to lose a recording.
    let mut conflicts = Vec::new();
    for entry in fs::read_dir(old_root)? {
        let name = entry?.file_name();
        if new_root.join(&name).exists() {
            conflicts.push(name.to_string_lossy().into_owned());
        }
    }
    if !conflicts.is_empty() {
        let noun = if conflicts.len() == 1 {
            "item"
        } else {
            "items"
        };
        return Err(UiError::app(
            "folder-conflict",
            format!(
                "\"{}\" already has {noun} named the same as something in your current meetings \
                 folder: {}. Rename or remove {noun} there first, then try again.",
                new_root.display(),
                conflicts.join(", "),
            ),
        ));
    }

    for entry in fs::read_dir(old_root)? {
        let entry = entry?;
        let from = entry.path();
        let to = new_root.join(entry.file_name());
        if fs::rename(&from, &to).is_err() {
            if entry.file_type()?.is_dir() {
                copy_dir(&from, &to)?;
                fs::remove_dir_all(&from)?;
            } else {
                fs::copy(&from, &to)?;
                fs::remove_file(&from)?;
            }
        }
    }
    // Best-effort: the folder is empty at this point on every platform this
    // ships on, but a leftover `.DS_Store` must not turn a successful move
    // into a reported failure.
    fs::remove_dir_all(old_root).ok();
    Ok(())
}

/// A recursive copy for the cross-volume fallback path. Std has no
/// `fs::copy` for directories.
fn copy_dir(from: &Path, to: &Path) -> Result<(), UiError> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}
