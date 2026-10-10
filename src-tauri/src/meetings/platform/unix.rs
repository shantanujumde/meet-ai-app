//! The macOS and Linux steps of copying the meetings folder (TUR-149).

use std::fs::File;
use std::io;
use std::path::Path;

/// Make `link` a symlink to `target`, exactly as the original stored it
/// (relative stays relative). Unix links have no file-or-folder flavour, so
/// `_points_at_dir` is only for Windows.
pub fn symlink(target: &Path, link: &Path, _points_at_dir: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Remove the symlink itself, never what it points at.
pub fn remove_symlink(link: &Path) -> io::Result<()> {
    std::fs::remove_file(link)
}

/// Flush a copied file to disk before the original is deleted.
///
/// Opened read-only: the copy keeps the original's permissions, which may
/// forbid writing, and `fsync` works on a read-only descriptor.
pub fn sync_file(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}
