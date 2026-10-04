//! The OS seam for running agent CLIs (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. What differs
//! per OS is small: how a child gets a process group of its own and how that
//! whole group is killed, what makes a file executable, and whether there is a
//! POSIX shell for the fake harness. Every unix shares `unix.rs`; macOS and
//! Linux re-export it, and Windows (with any other non-unix OS) has its own
//! stand-ins in `windows.rs`.

#[cfg(unix)]
mod unix;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(unix))]
mod windows;

#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "linux")]
use linux as os;

// Any other unix gets the shared unix code directly.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
use unix as os;

#[cfg(not(unix))]
use windows as os;

pub(crate) use os::{is_executable, kill_group, own_process_group};

// Where the CLIs install, per OS (TUR-53).
pub(crate) use os::{EXE_SUFFIXES, login_shell, search_dirs};

#[cfg(test)]
pub(crate) use os::{make_executable, search_dirs_under};

#[cfg(any(test, feature = "test-support"))]
pub(crate) use os::POSIX_SHELL;

// For the unix-only process tests, which check a pid with the same `kill`.
#[cfg(all(test, unix))]
pub(crate) use os::KILL;
