//! The one OS-specific step of deleting a meeting (TUR-116): how its folder
//! is handed to the system Trash (macOS), Recycle Bin (Windows) or the
//! freedesktop Trash (Linux). Everything that decides whether a meeting may
//! be deleted is in `super::delete`, and runs on every OS.
//!
//! Also the OS steps of moving the meetings folder (TUR-149): making and
//! removing a symlink, and flushing a copied file. The move itself is in
//! `super::root`.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::{remove_symlink, symlink, sync_file};
#[cfg(windows)]
pub use self::windows::{remove_symlink, symlink, sync_file};

use std::path::Path;

#[cfg(target_os = "macos")]
use self::macos::configure;
use crate::error::UiError;

/// What the user calls the place a deleted folder goes, for error messages.
const TRASH: &str = if cfg!(windows) {
    "Recycle Bin"
} else {
    "Trash"
};

/// Move `dir` to the system Trash. Nothing is erased: the user can take it
/// back out.
pub fn move_to_trash(dir: &Path) -> Result<(), UiError> {
    let mut context = trash::TrashContext::default();
    configure(&mut context);
    context.delete(dir).map_err(|error| {
        UiError::app(
            "trash-failed",
            format!("Could not move {} to the {TRASH}: {error}", dir.display()),
        )
    })
}

/// Windows and Linux have one way each; the crate's default is it.
#[cfg(not(target_os = "macos"))]
fn configure(_context: &mut trash::TrashContext) {}
