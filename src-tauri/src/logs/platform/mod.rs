//! Native crashes: a segfault, a bad instruction, an abort. Rust's panic hook
//! never sees these, so `crash-handler` catches them and [`write_native_file`]
//! leaves a short note next to the log. Nothing heavier: no minidump, no
//! upload, no analytics.
//!
//! The handler returns "not handled", so the OS's own crash handling (macOS's
//! crash reporter, Windows Error Reporting) still runs afterwards.

use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use super::crash::{Utc, header, unix_now, write_native_file};

// What the crash context holds differs per OS, so each describes its own.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// Describes the exception without allocating; see [`write_native_file`].
struct Exception<'a>(&'a crash_handler::CrashContext);

impl fmt::Display for Exception<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        os::describe(self.0, f)
    }
}

/// The OS log folder, as text, for a native crash note whose own folder is
/// gone (the meetings folder moved this launch, TUR-149). Set once from
/// `setup`; read in the crash handler, where a `OnceLock` read is one atomic
/// load and allocates nothing.
static FALLBACK: OnceLock<String> = OnceLock::new();

/// Remember the OS log folder for native crash notes. Skipped, with a
/// warning, when its path is not UTF-8 (see [`attach`]).
pub fn set_fallback(dir: &Path) {
    match dir.to_str() {
        Some(dir) => {
            let _ = FALLBACK.set(dir.to_owned());
        }
        None => {
            tracing::warn!(dir = %dir.display(), "the OS log folder's path is not UTF-8; native crashes after a folder move will not be written down");
        }
    }
}

// Adapted from github.com/thewh1teagle/vibe/desktop/src-tauri/src/setup.rs @ 53056d6bf3e5835c15fca3f1725f489de1e199f9 (MIT)
/// Attach the handler for the rest of the process's life.
pub fn attach(dir: &Path) {
    // Checked once here, so the crash path can build the file's path in a
    // stack buffer instead of allocating.
    let Some(dir) = dir.to_str().map(str::to_owned) else {
        tracing::warn!(dir = %dir.display(), "the logs folder's path is not UTF-8; native crashes will not be written down");
        return;
    };
    let header = header("native");
    // SAFETY: the closure only formats into stack buffers and writes one file
    // (see `write_native_file`); it takes no locks and does not allocate on
    // Unix, which is what a crashed process can still be trusted to do.
    let event = unsafe {
        crash_handler::make_crash_event(move |context: &crash_handler::CrashContext| {
            let at = Utc::from_unix(unix_now());
            let exception = Exception(context);
            // Never makes a folder: once the meetings folder has moved, `dir`
            // is gone and the note goes to the OS log folder instead.
            if write_native_file(&dir, &header, at, &exception).is_err()
                && let Some(fallback) = FALLBACK.get()
            {
                let _ = write_native_file(fallback, &header, at, &exception);
            }
            crash_handler::CrashEventResult::Handled(false)
        })
    };
    match crash_handler::CrashHandler::attach(event) {
        // Dropping the handler detaches it, so it is kept for as long as the
        // process runs. Vibe drops it at the end of `setup`, which turns it
        // straight back off.
        Ok(handler) => std::mem::forget(handler),
        Err(error) => tracing::warn!(%error, "could not attach the native crash handler"),
    }
}
