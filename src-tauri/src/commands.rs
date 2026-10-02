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
use crate::copy_prompt;
use crate::engine::{self, EnvironmentView, ModelView, SelectionView};
use crate::error::UiError;
use crate::folder_move::{self, FolderGate};
use crate::live_transcript::{LiveTranscript, Snapshot};
use crate::meetings::{self, Live, MeetingDetail, MeetingList};
use crate::onboarding;
use crate::permission;
use crate::recording::{Phase, Recorder, Status};
use crate::search;
use crate::tickets::{self, TicketSummary};
use crate::watch;

// --- off the main thread -------------------------------------------------

/// Run a command's blocking work — disk, a subprocess, the recorder — on
/// Tauri's blocking pool.
///
/// A plain `#[tauri::command]` runs on the main thread — the one AppKit draws
/// the window on — so every disk-touching command used to freeze the app while
/// it worked. `list_meetings` reads the WAV header of every meeting, and
/// `change_meetings_folder` can fall back to copying a whole folder tree across
/// volumes, which takes minutes. `#[tauri::command(async)]` alone would only
/// move that onto a tokio worker, and parking a worker for minutes starves the
/// other async commands, so the work goes to the pool built for blocking.
///
/// The closure must be `'static`, which is why these commands take an
/// `AppHandle` and look managed state up inside rather than borrowing a
/// `State<'_, T>` across the hop. A closure that returns a `Result` comes back
/// as `Result<Result<_>>`; callers flatten it with `?`.
async fn on_blocking_pool<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))
}

// --- meetings ------------------------------------------------------------

/// The recorder's state comes along so the meeting being recorded right now is
/// never labelled interrupted — mid-recording its files look exactly like a
/// killed one's (TUR-97).
#[tauri::command]
#[specta::specta]
pub async fn list_meetings(app: AppHandle) -> Result<MeetingList, UiError> {
    on_blocking_pool(move || {
        let status = app.state::<Recorder>().status();
        meetings::list(Live::from_status(&status))
    })
    .await?
}

#[tauri::command]
#[specta::specta]
pub async fn read_meeting(app: AppHandle, id: String) -> Result<MeetingDetail, UiError> {
    on_blocking_pool(move || {
        let status = app.state::<Recorder>().status();
        meetings::detail(&id, Live::from_status(&status))
    })
    .await?
}

/// Through the [`FolderGate`]: a note saved under the old root while the
/// folder is moving would be deleted with it.
#[tauri::command]
#[specta::specta]
pub async fn save_notes(app: AppHandle, id: String, body: String) -> Result<(), UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>()
            .writing(|| meetings::write_notes(&id, &body))?;
        // The folder watcher would report this write and the window would
        // reload under the cursor, so tell it the write was ours (TUR-100).
        if let Ok(root) = meetings::root() {
            watch::state(&app).note_own_write(&root.join(&id).join(store::NOTES_FILE));
        }
        Ok(())
    })
    .await?
}

/// Move the meetings folder somewhere else, taking every existing meeting
/// with it.
///
/// Refused while a recording is in flight: the recorder is mid-write to a
/// folder under the *old* root, and a move underneath it would either corrupt
/// that write or silently vanish the in-progress meeting.
///
/// Checking the phase once is not enough now that the move runs off the main
/// thread — the window stays live, and Record pressed a second into a
/// minutes-long copy would start a meeting under the old root just before
/// `move_contents` deletes it. The same goes for notes, the onboarding flag
/// and a model download. The [`FolderGate`] move guard is held for the whole
/// move, so every one of those writers is refused until it drops (see
/// [`folder_move`]). The phase check comes after the guard is taken: from then
/// on nothing can leave `Idle`, so the answer cannot go stale mid-move.
#[tauri::command]
#[specta::specta]
pub async fn change_meetings_folder(
    app: AppHandle,
    new_root: String,
) -> Result<MeetingList, UiError> {
    on_blocking_pool(move || {
        let gate = app.state::<FolderGate>();
        let _moving = gate.begin_move()?;
        if app.state::<Recorder>().status().phase != Phase::Idle {
            return Err(UiError::app(
                "recording-in-progress",
                "Stop the current recording before changing the meetings folder.",
            ));
        }
        let moved = meetings::change_root(PathBuf::from(new_root))?;
        // Watch the new folder instead of the old one (TUR-100).
        watch::state(&app).restart(&app);
        // ...and index it (TUR-101).
        search::state(&app).warm();
        Ok(moved)
    })
    .await?
}

/// Open a meeting's folder in Finder.
///
/// L7 makes the files the product, so "where is it on disk" is a first-class
/// question rather than a debugging affordance. Finding the folder reads the
/// meeting from disk, so it goes to the blocking pool with the rest.
#[tauri::command]
#[specta::specta]
pub async fn reveal_meeting(app: AppHandle, id: String) -> Result<(), UiError> {
    on_blocking_pool(move || {
        // Only the path is used, so which meeting is live does not matter here.
        let detail = meetings::detail(&id, Live::Nothing)?;
        app.opener()
            .open_path(&detail.path, None::<&str>)
            .map_err(|error| UiError::app("open-failed", error.to_string()))
    })
    .await?
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
#[specta::specta]
pub async fn measure_permission() -> permission::Status {
    tauri::async_runtime::spawn_blocking(permission::measure)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the permission measurement failed; falling back to the silent check");
            permission::status()
        })
}

/// The silent launch-time check — no chime (see `permission::quick`).
#[tauri::command]
#[specta::specta]
pub fn permission_quick() -> permission::Status {
    permission::quick()
}

/// Open System Settings at the pane the user needs.
///
/// Falls back to the Privacy & Security root if the anchored URL is refused,
/// which is the behaviour SPEC §8.1 asks for. The on-screen steps name the pane
/// as well, so the instructions still work even if both fail.
#[tauri::command]
#[specta::specta]
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
#[specta::specta]
pub async fn onboarding_state() -> Result<onboarding::State, UiError> {
    on_blocking_pool(onboarding::state).await?
}

/// Writes `.app/onboarding.json` under the root, so through the
/// [`FolderGate`] like every other writer there.
#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(app: AppHandle) -> Result<onboarding::State, UiError> {
    on_blocking_pool(move || app.state::<FolderGate>().writing(onboarding::complete)).await?
}

#[tauri::command]
#[specta::specta]
pub async fn reset_onboarding(app: AppHandle) -> Result<onboarding::State, UiError> {
    on_blocking_pool(move || app.state::<FolderGate>().writing(onboarding::reset)).await?
}

// --- engine and models ----------------------------------------------------

/// Filesystem-only, sub-millisecond. The settings route blocks on this.
///
/// `config::transcription` is itself a filesystem read, not a probe, so it
/// belongs on this side of the cheap/expensive split described in the module
/// doc comment. Cheap is still disk, though — the config file, the root
/// pointer, a stat per model — so it runs on the blocking pool like every other
/// command that touches the filesystem, not on the main thread.
#[tauri::command]
#[specta::specta]
pub async fn engine_environment() -> Result<EnvironmentView, UiError> {
    on_blocking_pool(|| {
        let transcription = config::transcription();
        engine::environment(engine::DEFAULT_LOCALE, &transcription.model)
    })
    .await
}

/// Runs `meet-stt --probe`, median ~160 ms. The settings route renders a
/// "Checking…" row and calls this after paint.
#[tauri::command]
#[specta::specta]
pub async fn engine_selection() -> Result<SelectionView, UiError> {
    // The probe spawns a process and waits on it, which would otherwise park a
    // tokio worker thread for the whole 160 ms.
    on_blocking_pool(|| {
        let transcription = config::transcription();
        engine::resolve(
            transcription.engine,
            engine::DEFAULT_LOCALE,
            &transcription.model,
        )
    })
    .await?
}

/// A stat per model under the meetings root, which means reading the root
/// pointer first — disk, so the blocking pool.
#[tauri::command]
#[specta::specta]
pub async fn model_catalogue() -> Result<Vec<ModelView>, UiError> {
    on_blocking_pool(engine::catalogue).await
}

/// Download a model, emitting `model://progress` as it goes.
///
/// One in-flight download per model: two would resume the same `.part` file
/// from two directions and race the atomic rename. The claim that enforces it
/// is taken inside `engine::download`, on the thread that writes the file, so
/// it lasts exactly as long as the writer does (see `engine::Claim`).
#[tauri::command]
#[specta::specta]
pub async fn download_model(app: AppHandle, id: String) -> Result<String, UiError> {
    engine::download(app, id).await
}

// --- recording ------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn recording_status(recorder: State<'_, Recorder>) -> Status {
    recorder.status()
}

/// Starting or stopping blocks on real wall-clock time — SPEC §8.1's
/// positive-control permission measurement on start, Core Audio warming up or
/// winding down either side — so both run on a blocking thread rather than
/// parking a tokio worker, the same reason `measure_permission` does.
///
/// The toggle goes through the [`FolderGate`], the same as ⌘⇧R and the menu
/// bar, so the button cannot start a recording while the folder is moving.
#[tauri::command]
#[specta::specta]
pub async fn toggle_recording(app: AppHandle) -> Result<Status, UiError> {
    on_blocking_pool(move || folder_move::toggle_recording(&app)).await?
}

/// Stopping is never gated: a move only runs while nothing is recording, so
/// there is nothing for a stop to race.
#[tauri::command]
#[specta::specta]
pub async fn stop_recording(app: AppHandle) -> Result<Status, UiError> {
    on_blocking_pool(move || app.state::<Recorder>().stop(&app)).await?
}

// --- live transcript ------------------------------------------------------

/// Everything the live pane should show right now, so a window opened
/// mid-meeting (or reloaded) catches up without replaying events it missed.
/// In-memory only and cheap; after this, `transcript://update` and
/// `transcript://status` keep it current, deduplicated by `seq`.
#[tauri::command]
#[specta::specta]
pub fn live_transcript(live: State<'_, LiveTranscript>) -> Snapshot {
    live.snapshot()
}

/// Every ticket in the meetings folder, newest first (TUR-102).
#[tauri::command]
#[specta::specta]
pub async fn list_tickets() -> Result<Vec<TicketSummary>, UiError> {
    on_blocking_pool(tickets::list).await?
}

/// Add a ticket by hand. Through the [`FolderGate`], like [`save_notes`].
#[tauri::command]
#[specta::specta]
pub async fn create_ticket(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<TicketSummary, UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>()
            .writing(|| tickets::create(&title, &body))
    })
    .await?
}

/// The Start Work prompt for a ticket, for the window to copy (SPEC L14).
/// `meeting_id` is the meeting the ticket is listed under, if any.
#[tauri::command]
#[specta::specta]
pub async fn start_work_prompt(
    ticket_id: String,
    meeting_id: Option<String>,
) -> Result<String, UiError> {
    on_blocking_pool(move || copy_prompt::start_work(&ticket_id, meeting_id.as_deref())).await?
}

/// The clipboard wrap-up prompt for a meeting, for when no agent is set up
/// (SPEC A11).
#[tauri::command]
#[specta::specta]
pub async fn wrap_up_prompt(meeting_id: String) -> Result<String, UiError> {
    on_blocking_pool(move || copy_prompt::wrap_up(&meeting_id)).await?
}

/// Whether the meeting view offers Copy prompt: `agent.harness` is `none`.
#[tauri::command]
#[specta::specta]
pub async fn copy_prompt_fallback() -> Result<bool, UiError> {
    on_blocking_pool(copy_prompt::fallback).await?
}
