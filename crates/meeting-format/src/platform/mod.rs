//! The OS seam for `meeting-format` (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. One thing
//! differs per OS: whether a folder can be `fsync`ed so a rename inside it
//! survives a power cut ([`sync_dir`]). Every unix (macOS, Linux, the rest)
//! shares one implementation in `unix.rs`; Windows has its own no-op.

#[cfg(unix)]
mod unix;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub(crate) use macos::sync_dir;

#[cfg(target_os = "linux")]
pub(crate) use linux::sync_dir;

#[cfg(windows)]
pub(crate) use windows::sync_dir;

#[cfg(windows)]
pub(crate) use windows::rename;

#[cfg(windows)]
pub(crate) use windows::is_lock_violation;

/// Every OS but Windows: no extra codes mean "another program holds it".
#[cfg(not(windows))]
pub(crate) fn is_lock_violation(_code: i32) -> bool {
    false
}

/// Every OS but Windows: a plain `rename(2)`. Unix renames over a file
/// another program holds open, so there is nothing to retry.
#[cfg(not(windows))]
pub(crate) fn rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

// Any other unix gets the shared unix code directly.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub(crate) use unix::sync_dir;

// Neither unix nor Windows: no folder handle to flush that we know of.
#[cfg(not(any(unix, windows)))]
pub(crate) fn sync_dir(_dir: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}
