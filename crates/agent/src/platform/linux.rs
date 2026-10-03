//! Agent CLIs on Linux: the shared unix process-group, executable and shell code.

#[cfg(test)]
pub(crate) use super::unix::KILL;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use super::unix::POSIX_SHELL;
pub(crate) use super::unix::{is_executable, kill_group, own_process_group};
