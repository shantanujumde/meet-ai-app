//! Folder `fsync`, shared by every unix.

use std::fs::File;
use std::io;
use std::path::Path;

/// `fsync` a folder so a rename inside it survives a power cut.
///
/// A filesystem that cannot sync a directory at all (some network mounts
/// answer `EINVAL`) is treated as done — the rename already happened, and
/// failing the whole write over a durability hint the filesystem does not
/// support would turn a saved file into an error.
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    match File::open(dir).and_then(|dir| dir.sync_all()) {
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        other => other,
    }
}
