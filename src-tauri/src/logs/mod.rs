//! Where the log and the crash files go (SPEC §3.1: `.app/logs/meet-ai.log`).
//!
//! One folder a user can open from Settings and attach to a bug report: the
//! live log, at most one rotated copy of it, and up to [`MAX_CRASH_FILES`]
//! short crash files. Nothing here is ever sent anywhere (SPEC §8.1, no
//! telemetry); a crash file is only read by whoever the user hands it to.
//!
//! Before onboarding there is no meetings root yet, so the log goes to the OS
//! log folder as it always did (`~/Library/Logs/<id>` on macOS); the first
//! launch after onboarding switches to `.app/logs`.

use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::{AppHandle, Manager as _, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, log};
use tauri_plugin_opener::OpenerExt as _;

use crate::error::{UiError, on_blocking_pool};

mod crash;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod platform;

pub use crash::install_panic_hook;

/// The log file's name without `.log`; the plugin adds the extension.
pub const LOG_FILE_STEM: &str = "meet-ai";
/// The live log rotates once it reaches this size. With one rotated copy kept,
/// the log never takes more than about twice this on disk.
pub const LOG_MAX_BYTES: u128 = 1_000_000;
/// Crash files kept in the folder; older ones are deleted first.
pub const MAX_CRASH_FILES: usize = 5;

/// `<meetings root>/.app/logs`, once onboarding is done. `None` before that,
/// so a first launch does not create `~/Meetings` before the user has chosen
/// where meetings go.
pub fn meetings_logs_dir() -> Option<PathBuf> {
    let onboarded = crate::onboarding::state()
        .ok()
        .is_some_and(|state| state.completed_at.is_some());
    if !onboarded {
        return None;
    }
    crate::meetings::root()
        .ok()
        .map(|root| meeting_format::layout::logs_dir(&root))
}

/// The folder the log is actually written to this launch: the meetings one,
/// or the OS log folder before onboarding. Read once at startup, so a folder
/// move or a finished onboarding takes effect on the next launch, the same
/// as the plugin's file target.
pub struct LogsDir(pub Option<PathBuf>);

/// Where this launch's log and crash files are.
pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.try_state::<LogsDir>()
        .and_then(|state| state.0.clone())
        .or_else(|| app.path().app_log_dir().ok())
}

// Adapted from github.com/cjpais/Handy/src-tauri/src/lib.rs @ ffbc9504cbf004ce4819d2ca872fcea92be0fddf (MIT)
/// The tauri-plugin-log builder: one size-capped file, plus the terminal and
/// the webview console in debug builds only.
pub fn plugin<R: Runtime>(dir: Option<PathBuf>) -> tauri::plugin::TauriPlugin<R> {
    let file = match dir {
        Some(path) => TargetKind::Folder {
            path,
            file_name: Some(LOG_FILE_STEM.into()),
        },
        None => TargetKind::LogDir {
            file_name: Some(LOG_FILE_STEM.into()),
        },
    };
    let mut targets = vec![Target::new(file)];
    if cfg!(debug_assertions) {
        targets.push(Target::new(TargetKind::Stdout));
        targets.push(Target::new(TargetKind::Webview));
    }
    tauri_plugin_log::Builder::new()
        .level(log::LevelFilter::Info)
        // tao and wry log every AppKit callback at TRACE. Left alone they bury
        // our own lines under thousands of theirs, which makes a
        // user-submitted log file useless.
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .max_file_size(LOG_MAX_BYTES)
        // Keeps the live file plus one rotated copy (`meet-ai_<date>.log`), so
        // the log takes at most 2 x LOG_MAX_BYTES on disk. `KeepOne` would
        // delete the full file outright and lose the lines just before
        // whatever went wrong.
        .rotation_strategy(RotationStrategy::KeepSome(1))
        .clear_targets()
        .targets(targets)
        .build()
}

/// The OS log folder, where a panic's file goes once this launch's logs
/// folder is gone (the meetings folder moved). Set by [`set_crash_fallback`].
static CRASH_FALLBACK: OnceLock<PathBuf> = OnceLock::new();

/// Remember the OS log folder for crash files, and make it, so the panic path
/// never has to create a folder (TUR-90). Called once from `setup`.
pub fn set_crash_fallback<R: Runtime>(app: &AppHandle<R>) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%error, dir = %dir.display(), "could not create the OS log folder");
    }
    let _ = CRASH_FALLBACK.set(dir);
}

/// Install the panic hook and the native crash handler, writing into `dir`.
///
/// Called once from `setup`, after the log plugin, so the one line saying
/// where crash files go lands in the log itself.
pub fn install_crash_handlers(dir: PathBuf) {
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%error, dir = %dir.display(), "could not create the logs folder");
    }
    crash::prune(&dir, MAX_CRASH_FILES);
    install_panic_hook(dir.clone(), || CRASH_FALLBACK.get().cloned());
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    platform::attach(&dir);
    tracing::info!(dir = %dir.display(), "logs and crash files go here");
}

/// Open the logs folder in Finder (Explorer, the file manager on Linux), so
/// the user can find the one file to attach to a bug report.
#[tauri::command]
#[specta::specta]
pub async fn open_logs_folder(app: AppHandle) -> Result<(), UiError> {
    on_blocking_pool(move || {
        let dir = resolve(&app).ok_or_else(|| {
            UiError::app(
                "no-logs-dir",
                "meet-ai could not work out where its logs folder is.",
            )
        })?;
        std::fs::create_dir_all(&dir)?;
        app.opener()
            .open_path(dir.display().to_string(), None::<&str>)
            .map_err(|error| UiError::app("open-failed", error.to_string()))
    })
    .await?
}

#[cfg(test)]
mod tests;
