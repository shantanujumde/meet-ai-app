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

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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

/// The folder the log plugin was pointed at when this launch started: the
/// meetings one, or `None` for the OS log folder before onboarding. The
/// plugin's file target cannot change while the app runs.
pub struct LogsDir(pub Option<PathBuf>);

/// Where the logs and crash files are now, worked out each time it is asked
/// rather than kept from startup: after a folder move, the startup folder is
/// under a root the user moved away from, and opening it would make that
/// folder again (TUR-149).
pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let launched_in_meetings = app
        .try_state::<LogsDir>()
        .is_some_and(|state| state.0.is_some());
    pick_dir(
        launched_in_meetings,
        current_meetings_logs_dir(),
        app.path().app_log_dir().ok(),
    )
}

/// The logs folder under the meetings root as it is now, if that root is
/// there. `None` before onboarding, or when the root is gone.
fn current_meetings_logs_dir() -> Option<PathBuf> {
    let dir = meetings_logs_dir()?;
    crate::meetings::root()
        .is_ok_and(|root| root.is_dir())
        .then_some(dir)
}

/// The rule behind [`resolve`]. A launch that started before onboarding logs
/// to the OS folder until it restarts, so that is where its log is. Otherwise
/// the current meetings root's logs folder, falling back to the OS log folder
/// when the root is not there; never a folder under an old root.
fn pick_dir(
    launched_in_meetings: bool,
    meetings: Option<PathBuf>,
    os: Option<PathBuf>,
) -> Option<PathBuf> {
    match (launched_in_meetings, meetings) {
        (true, Some(dir)) => Some(dir),
        _ => os,
    }
}

/// Where crash files go after a folder move this launch. Read by the panic
/// hook; `None` until a move.
static MOVED_LOGS_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The meetings folder moved to `root`: send this launch's crash files to its
/// logs folder from now on (TUR-149). Makes no folder; a crash with no logs
/// folder there goes to the OS log folder instead (see `crash::panic_dir`).
pub fn follow_root(root: &Path) {
    let dir = meeting_format::layout::logs_dir(root);
    *MOVED_LOGS_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir);
}

/// The folder a panic's file goes in: the moved-to one after a move, else the
/// one the handlers were installed with. `try_lock`, so a panic while the
/// lock is held cannot deadlock the hook.
fn panic_dir_now(installed: &Path) -> PathBuf {
    MOVED_LOGS_DIR
        .try_lock()
        .ok()
        .and_then(|moved| moved.clone())
        .unwrap_or_else(|| installed.to_path_buf())
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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    platform::set_fallback(&dir);
    let _ = CRASH_FALLBACK.set(dir);
}

/// Set once the crash handlers are in, so a second call adds no second hook
/// (two hooks would write every panic twice).
static CRASH_HANDLERS: OnceLock<()> = OnceLock::new();

/// Install the crash handlers at the very top of `run()`, before Tauri or the
/// log plugin exist, so a crash during startup still leaves a crash file
/// (TUR-125). Logs nothing: there is no logger yet.
///
/// Only called once onboarding is done. Before that there is no logs folder
/// to write into, so a crash before `setup` relies on the OS crash report.
pub fn install_early(dir: PathBuf) {
    install_once(&CRASH_HANDLERS, || install_handlers(&dir));
}

/// Install the panic hook and the native crash handler, writing into `dir`,
/// unless [`install_early`] already did.
///
/// Called once from `setup`, after the log plugin, so the one line saying
/// where crash files go lands in the log itself.
pub fn install_crash_handlers(dir: PathBuf) {
    install_once(&CRASH_HANDLERS, || install_handlers(&dir));
    tracing::info!(dir = %dir.display(), "logs and crash files go here");
}

/// Run `install` only the first time `flag` is seen.
fn install_once(flag: &OnceLock<()>, install: impl FnOnce()) {
    let mut first = false;
    flag.get_or_init(|| first = true);
    if first {
        install();
    }
}

fn install_handlers(dir: &std::path::Path) {
    if let Err(error) = std::fs::create_dir_all(dir) {
        tracing::warn!(%error, dir = %dir.display(), "could not create the logs folder");
    }
    crash::prune(dir, MAX_CRASH_FILES);
    let installed = dir.to_path_buf();
    install_panic_hook(
        move || panic_dir_now(&installed),
        || CRASH_FALLBACK.get().cloned(),
    );
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    platform::attach(dir);
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
        // `resolve` only names a folder under a root that is there, or the
        // OS log folder, so this never makes an old meetings root again.
        std::fs::create_dir_all(&dir)?;
        app.opener()
            .open_path(dir.display().to_string(), None::<&str>)
            .map_err(|error| UiError::app("open-failed", error.to_string()))
    })
    .await?
}

#[cfg(test)]
mod tests;
