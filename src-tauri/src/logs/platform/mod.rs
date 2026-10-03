//! Native crashes: a segfault, a bad instruction, an abort. Rust's panic hook
//! never sees these, so `crash-handler` catches them and [`write_native_file`]
//! leaves a short note next to the log. Nothing heavier: no minidump, no
//! upload, no analytics.
//!
//! The handler returns "not handled", so the OS's own crash handling (macOS's
//! crash reporter, Windows Error Reporting) still runs afterwards.

use std::fmt;
use std::path::Path;

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
            let _ = write_native_file(
                &dir,
                &header,
                Utc::from_unix(unix_now()),
                &Exception(context),
            );
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
