//! The Windows steps of copying the meetings folder (TUR-149).

use std::fs::{self, OpenOptions};
use std::io;
use std::os::windows::fs::FileTypeExt as _;
use std::path::Path;

/// Make `link` a symlink to `target`. Windows has separate file and folder
/// links, so the caller says which the original was.
///
/// Making a symlink needs Developer Mode or an elevated process on Windows
/// (verify it on a real machine). Without that this fails, and the move takes
/// back what it copied and stops, leaving every meeting where it was.
pub fn symlink(target: &Path, link: &Path, points_at_dir: bool) -> io::Result<()> {
    if points_at_dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// Remove the symlink itself, never what it points at. A folder link is
/// removed like a folder, a file link like a file.
pub fn remove_symlink(link: &Path) -> io::Result<()> {
    if fs::symlink_metadata(link)?.file_type().is_symlink_dir() {
        fs::remove_dir(link)
    } else {
        fs::remove_file(link)
    }
}

/// Flush a copied file to disk before the original is deleted.
///
/// `FlushFileBuffers` needs a handle with write access. A copy that kept a
/// read-only attribute cannot be opened that way, and is left to the OS to
/// flush rather than failing the whole move over it.
pub fn sync_file(path: &Path) -> io::Result<()> {
    match OpenOptions::new().write(true).open(path) {
        Ok(file) => file.sync_all(),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(error),
    }
}
