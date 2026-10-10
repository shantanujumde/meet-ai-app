//! Checking a meetings-folder move's two paths before anything moves
//! (TUR-149).
//!
//! The checks compare real locations, not the text of the paths: a symlink
//! into the current folder, or the same folder typed in another case on a
//! case-insensitive disk, would otherwise pass the "not inside itself" check,
//! and a copy into itself fills the disk.

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::UiError;

/// The real locations of the current and the new meetings folder, once the
/// move between them is known to be safe to start.
pub(super) fn checked(old_root: &Path, new_root: &Path) -> Result<(PathBuf, PathBuf), UiError> {
    let new_root = checked_alone(new_root)?;
    let old_root = canonical_nearest(old_root)?;
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
    Ok((old_root, new_root))
}

/// The real location of a folder the user picked: a full path, with no `..`
/// in it, symlinks resolved.
pub(super) fn checked_alone(new_root: &Path) -> Result<PathBuf, UiError> {
    let relative = !new_root.is_absolute()
        || new_root
            .components()
            .any(|part| matches!(part, Component::ParentDir));
    if relative {
        return Err(UiError::app(
            "relative-folder",
            format!(
                "\"{}\" is not a full folder path. Pick the folder again.",
                new_root.display()
            ),
        ));
    }
    Ok(canonical_nearest(new_root)?)
}

/// `path` with every symlink resolved and, on a case-insensitive disk, each
/// name in the case it is stored in. The part of `path` that does not exist
/// yet (a new folder the move will make) is kept as written, under the real
/// location of the deepest part that does.
fn canonical_nearest(path: &Path) -> io::Result<PathBuf> {
    let mut missing = Vec::new();
    let mut at = path;
    loop {
        match dunce::canonicalize(at) {
            Ok(mut real) => {
                real.extend(missing.iter().rev());
                return Ok(real);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let (Some(name), Some(parent)) = (at.file_name(), at.parent()) else {
                    return Err(error);
                };
                missing.push(name.to_owned());
                at = parent;
            }
            Err(error) => return Err(error),
        }
    }
}
