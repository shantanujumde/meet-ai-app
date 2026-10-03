//! Agent CLIs on Windows (and any other non-unix OS): stand-ins until the
//! Windows parity ticket.
//!
//! There are no process groups here yet, so a run's time limit or Cancel kills
//! the child itself (`process::stop`) and not what it started; the port can
//! put the child in a Job Object instead. Execute permission is not a file
//! bit on Windows, so any regular file counts.

use std::path::Path;
use std::process::Command;

/// No POSIX shell to run the fake harness's script with.
#[cfg(any(test, feature = "test-support"))]
pub(crate) const POSIX_SHELL: Option<&str> = None;

/// No process groups to join outside unix.
pub(crate) fn own_process_group(_command: &mut Command) {}

/// There are no process groups to kill outside unix; `process::stop` kills
/// the child itself.
pub(crate) fn kill_group(_pid: u32) {}

/// Any regular file.
pub(crate) fn is_executable(path: &Path) -> bool {
    path.is_file()
}
