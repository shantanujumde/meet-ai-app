//! The IPC surface: every `invoke` the webview can make, in one place.
//!
//! These are thin on purpose. Each one is an argument check, a call into a
//! module beside this one, and a conversion into [`UiError`]. Logic that lives
//! here instead of in a module is logic with no unit test, because a
//! `#[tauri::command]` needs an `AppHandle` to call.
//!
//! The matching TypeScript wrappers are in `src/ipc/client.ts`; the two files
//! are a pair and should be edited together.

use std::path::PathBuf;

use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_opener::OpenerExt as _;

use crate::config;
use crate::engine::{self, Downloads, EnvironmentView, ModelView, SelectionView};
use crate::error::UiError;
use crate::live_transcript::{LiveTranscript, Snapshot};
use crate::meetings::{self, Live, MeetingDetail, MeetingList};
use crate::onboarding;
use crate::permission;
use crate::recording::{Phase, Recorder, Status};

// --- off the main thread -------------------------------------------------

/// Run a command's disk work on Tauri's blocking pool.
///
/// A plain `#[tauri::command]` runs on the main thread — the one AppKit draws
/// the window on — so every one of these used to freeze the app while it
/// worked. `list_meetings` reads the WAV header of every meeting, and
/// `change_meetings_folder` can fall back to copying a whole folder tree across
/// volumes, which takes minutes. `#[tauri::command(async)]` alone would only
/// move that onto a tokio worker, and parking a worker for minutes starves the
/// other async commands, so the work goes to the pool built for blocking.
///
/// The closure must be `'static`, which is why these commands take an
/// `AppHandle` and look the recorder up inside rather than borrowing a
/// `State<'_, Recorder>` across the hop.
async fn on_blocking_pool<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, UiError> + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| UiError::app("disk-task-failed", error.to_string()))?
}

// --- meetings ------------------------------------------------------------

/// The recorder's state comes along so the meeting being recorded right now is
/// never labelled interrupted — mid-recording its files look exactly like a
/// killed one's (TUR-97).
#[tauri::command]
pub async fn list_meetings(app: AppHandle) -> Result<MeetingList, UiError> {
    on_blocking_pool(move || {
        let status = app.state::<Recorder>().status();
        meetings::list(Live::from_status(&status))
    })
    .await
}

#[tauri::command]
pub async fn read_meeting(app: AppHandle, id: String) -> Result<MeetingDetail, UiError> {
    on_blocking_pool(move || {
        let status = app.state::<Recorder>().status();
        meetings::detail(&id, Live::from_status(&status))
    })
    .await
}

#[tauri::command]
pub async fn save_notes(id: String, body: String) -> Result<(), UiError> {
    on_blocking_pool(move || meetings::write_notes(&id, &body)).await
}

/// Move the meetings folder somewhere else, taking every existing meeting
/// with it.
///
/// Refused while a recording is in flight: the recorder is mid-write to a
/// folder under the *old* root, and a move underneath it would either corrupt
/// that write or silently vanish the in-progress meeting. The phase is read on
/// the pool thread, right before the move, rather than before the hop, so the
/// check sits as close to the move as it did when this ran inline.
#[tauri::command]
pub async fn change_meetings_folder(
    app: AppHandle,
    new_root: String,
) -> Result<MeetingList, UiError> {
    on_blocking_pool(move || {
        if app.state::<Recorder>().status().phase != Phase::Idle {
            return Err(UiError::app(
                "recording-in-progress",
                "Stop the current recording before changing the meetings folder.",
            ));
        }
        meetings::change_root(PathBuf::from(new_root))
    })
    .await
}

/// Open a meeting's folder in Finder.
///
/// L7 makes the files the product, so "where is it on disk" is a first-class
/// question rather than a debugging affordance. Finding the folder reads the
/// meeting from disk, so it goes to the blocking pool with the rest.
#[tauri::command]
pub async fn reveal_meeting(app: AppHandle, id: String) -> Result<(), UiError> {
    on_blocking_pool(move || {
        // Only the path is used, so which meeting is live does not matter here.
        let detail = meetings::detail(&id, Live::Nothing)?;
        app.opener()
            .open_path(&detail.path, None::<&str>)
            .map_err(|error| UiError::app("open-failed", error.to_string()))
    })
    .await
}

// --- permission and onboarding -------------------------------------------

/// Runs the real positive-control check (SPEC §8.1/A6): plays the permission
/// chime and probes the microphone. Real wall-clock time, so it runs on a
/// blocking thread rather than parking a tokio worker.
///
/// If the measurement itself dies (a panic on the blocking thread), the answer
/// falls back to the silent check rather than failing the screen — but it is
/// logged, because otherwise a broken measurement looks exactly like a working
/// one.
#[tauri::command]
pub async fn permission_status() -> permission::Status {
    tauri::async_runtime::spawn_blocking(permission::measure)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the permission measurement failed; falling back to the silent check");
            permission::status()
        })
}

/// The silent launch-time check — no chime (see `permission::quick`).
#[tauri::command]
pub fn permission_quick() -> permission::Status {
    permission::quick()
}

/// Open System Settings at the pane the user needs.
///
/// Falls back to the Privacy & Security root if the anchored URL is refused,
/// which is the behaviour SPEC §8.1 asks for. The on-screen steps name the pane
/// as well, so the instructions still work even if both fail.
#[tauri::command]
pub fn open_privacy_settings(app: AppHandle, pane: permission::Pane) -> Result<(), UiError> {
    match app.opener().open_url(pane.url(), None::<&str>) {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::warn!(%error, url = pane.url(), "anchored settings link failed; falling back to the pane root");
            app.opener()
                .open_url(permission::PRIVACY_ROOT_URL, None::<&str>)
                .map_err(|error| {
                    UiError::app(
                        "open-failed",
                        format!("meet-ai could not open System Settings: {error}"),
                    )
                })
        }
    }
}

/// The onboarding flag is a file under the meetings root (see
/// [`onboarding`]), so all three go to the blocking pool with the meetings
/// commands. `onboarding_state` is on the launch path; a slow or sleeping disk
/// must not hold the first paint.
#[tauri::command]
pub async fn onboarding_state() -> Result<onboarding::State, UiError> {
    on_blocking_pool(onboarding::state).await
}

#[tauri::command]
pub async fn complete_onboarding() -> Result<onboarding::State, UiError> {
    on_blocking_pool(onboarding::complete).await
}

#[tauri::command]
pub async fn reset_onboarding() -> Result<onboarding::State, UiError> {
    on_blocking_pool(onboarding::reset).await
}

// --- engine and models ----------------------------------------------------

/// Filesystem-only, sub-millisecond. The settings route blocks on this.
///
/// `config::transcription` is itself a filesystem read, not a probe, so it
/// belongs on this side of the cheap/expensive split described in the module
/// doc comment.
#[tauri::command]
pub fn engine_environment() -> EnvironmentView {
    let transcription = config::transcription();
    engine::environment(engine::DEFAULT_LOCALE, &transcription.model)
}

/// Runs `meet-stt --probe`, median ~160 ms. The settings route renders a
/// "Checking…" row and calls this after paint.
#[tauri::command]
pub async fn engine_selection() -> Result<SelectionView, UiError> {
    // The probe spawns a process and waits on it, which would otherwise park a
    // tokio worker thread for the whole 160 ms.
    tauri::async_runtime::spawn_blocking(|| {
        let transcription = config::transcription();
        engine::resolve(
            transcription.engine,
            engine::DEFAULT_LOCALE,
            &transcription.model,
        )
    })
    .await
    .map_err(|error| UiError::app("probe-failed", error.to_string()))?
}

#[tauri::command]
pub fn model_catalogue() -> Vec<ModelView> {
    engine::catalogue()
}

/// Download a model, emitting `model://progress` as it goes.
///
/// The claim is taken here rather than inside `engine::download` so the
/// managed `Downloads` state never has to cross into the `'static` task that
/// does the work. One in-flight download per model: two would resume the same
/// `.part` file from two directions and race the atomic rename.
#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    downloads: State<'_, Downloads>,
    id: String,
) -> Result<String, UiError> {
    // Held, not just checked: the claim is given back when `_claim` drops at
    // the end of this function, even if the download panics or this future is
    // dropped part-way through.
    let Some(_claim) = downloads.claim(&id) else {
        return Err(UiError::app(
            "download-already-running",
            "That model is already downloading.",
        ));
    };
    engine::download(app, id).await
}

// --- recording ------------------------------------------------------------

#[tauri::command]
pub fn recording_status(recorder: State<'_, Recorder>) -> Status {
    recorder.status()
}

/// Starting or stopping blocks on real wall-clock time — SPEC §8.1's
/// positive-control permission measurement on start, Core Audio warming up or
/// winding down either side — so both run on a blocking thread rather than
/// parking a tokio worker, the same reason `permission_status` does.
#[tauri::command]
pub async fn toggle_recording(app: AppHandle) -> Result<Status, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let recorder = app.state::<Recorder>();
        recorder.toggle(&app)
    })
    .await
    .map_err(|error| UiError::app("recorder-task-failed", error.to_string()))?
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> Result<Status, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let recorder = app.state::<Recorder>();
        recorder.stop(&app)
    })
    .await
    .map_err(|error| UiError::app("recorder-task-failed", error.to_string()))?
}

// --- live transcript ------------------------------------------------------

/// Everything the live pane should show right now, so a window opened
/// mid-meeting (or reloaded) catches up without replaying events it missed.
/// In-memory only and cheap; after this, `transcript://update` and
/// `transcript://status` keep it current, deduplicated by `seq`.
#[tauri::command]
pub fn live_transcript(live: State<'_, LiveTranscript>) -> Snapshot {
    live.snapshot()
}
