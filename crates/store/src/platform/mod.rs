//! The OS seam for `store` (SPEC §8.2, quality rule R10).
//!
//! This is the only file in the crate that names an operating system. Each OS
//! gets one module with the same functions, and the rest of the crate calls
//! `crate::platform::…` without a `cfg` of its own:
//!
//! * Windows: `windows.rs` (its sharing and lock violation error codes).
//! * Every other OS: `other.rs`, plain `std` only.
//!
//! `store` is otherwise OS-free: everything else is plain `std::fs` and
//! `Path::join`.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as os;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
use other as os;

/// Whether the raw OS error `code` means "another program holds this file",
/// beyond the `PermissionDenied` kind every OS reports. Retention skips such a
/// WAV and tries again on its next run.
pub(crate) fn is_lock_violation(code: i32) -> bool {
    os::is_lock_violation(code)
}

/// Hide `<root>/.app` where a dot name does not (Windows). Best effort: a
/// failure is logged, never fatal; a missing folder is left alone.
pub(crate) fn hide_app_dir(dir: &std::path::Path) {
    os::hide_app_dir(dir)
}

/// Test-only: make a meeting's WAVs undeletable the way this OS does it.
#[cfg(test)]
pub(crate) use os::test_lock;
