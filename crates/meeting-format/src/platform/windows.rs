//! `meeting-format` on Windows.

use std::io;
use std::path::Path;

/// A no-op: Windows has no directory handle to flush, and `MoveFileEx` is
/// already as durable as that platform offers.
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}
