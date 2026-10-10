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
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::{self, FolderGate};
use crate::live_transcript::{LiveTranscript, Snapshot};
use crate::meetings::{self, Live, MeetingDetail, MeetingList};
use crate::onboarding;
use crate::permission;
use crate::recording::{Phase, Recorder, Status};
use crate::search;
use crate::tickets::{self, TicketSummary};
use crate::watch;

// --- meetings ------------------------------------------------------------

/// The recorder's state comes along so the meeting being recorded right now is
/// never labelled interrupted — mid-recording its files look exactly like a
/// killed one's (TUR-97).
#[tauri::command]
#[specta::specta]
pub async fn list_meetings(app: AppHandle) -> Result<MeetingList, UiError> {
    on_blocking_pool(move || {
        // The folder may have appeared since launch (first recording): start
        // watching it now (TUR-122).
        watch::state(&app).ensure_running(&app);
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
        // The watcher skips that write, so search learns the notes here (TUR-152).
        search::meeting_written(&app, &id);
        Ok(())
    })
    .await?
}

/// Rename a meeting from its page (TUR-103), and answer with the title as
/// written: one line, trimmed, at most `store::meeting_title::MAX_TITLE_CHARS`
/// characters. A blank title is refused.
///
/// Through the [`FolderGate`], like [`save_notes`]. Not noted as our own
/// write, so the watcher still tells the window. The search index is updated
/// here, as it is after the calendar's and the agent's titles (TUR-107).
#[tauri::command]
#[specta::specta]
pub async fn rename_meeting(app: AppHandle, id: String, title: String) -> Result<String, UiError> {
    on_blocking_pool(move || {
        let saved = app
            .state::<FolderGate>()
            .writing(|| meetings::rename(&id, &title))?;
        search::meeting_written(&app, &id);
        Ok(saved)
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
        // Write the kept Sync issues the move refused to let through (TUR-21).
        app.state::<crate::sync::SyncRuns>().after_folder_move();
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

/// Open the OS settings page the user needs (TUR-51: per OS, see
/// [`crate::settings_links`]).
///
/// On macOS that is the anchored Privacy & Security pane, falling back to the
/// pane root if the anchor is refused, which is the behaviour SPEC §8.1 asks
/// for. The on-screen steps name the pane as well, so the instructions still
/// work even if both fail.
#[tauri::command]
#[specta::specta]
pub fn open_privacy_settings(app: AppHandle, pane: permission::Pane) -> Result<(), UiError> {
    let mut last_error = "this system has no settings page for it".to_string();
    for target in crate::settings_links::targets_here(pane) {
        let opened = match &target {
            crate::settings_links::Target::Url(url) => app
                .opener()
                .open_url(*url, None::<&str>)
                .map_err(|error| error.to_string()),
            crate::settings_links::Target::Command(program, args) => {
                std::process::Command::new(program)
                    .args(args)
                    .spawn()
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        };
        match opened {
            Ok(()) => return Ok(()),
            Err(error) => {
                tracing::warn!(%error, ?target, "settings link failed; trying the next one");
                last_error = error;
            }
        }
    }
    Err(UiError::app(
        "open-failed",
        format!(
            "meet-ai could not open {}: {last_error}",
            crate::settings_links::settings_name(std::env::consts::OS)
        ),
    ))
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

/// Delete a downloaded whisper model that is not picked and not in use
/// (TUR-132). Refuses with `model-in-use` otherwise.
#[tauri::command]
#[specta::specta]
pub async fn delete_model(app: AppHandle, id: String) -> Result<(), UiError> {
    on_blocking_pool(move || engine::delete(&app, &id)).await?
}

/// The Settings engine picker (TUR-75): the saved choice, what "Automatic"
/// lands on, and which choices this Mac can run. Runs the ~160 ms probe.
#[tauri::command]
#[specta::specta]
pub async fn engine_choices() -> Result<engine::EngineChoices, UiError> {
    on_blocking_pool(engine::choices).await
}

/// The model licences Settings, About must credit (TUR-62: Parakeet's
/// CC-BY-4.0). Constant data, no disk.
#[tauri::command]
#[specta::specta]
pub fn model_credits() -> Vec<engine::ModelCredit> {
    engine::credits()
}

/// Save `transcription.engine` and `transcription.model` into config.jsonc,
/// keeping the rest of the file. Takes effect on the next recording. Writes
/// under the meetings root, so through the [`FolderGate`].
#[tauri::command]
#[specta::specta]
pub async fn set_transcription(
    app: AppHandle,
    engine: engine::EngineChoice,
    model: String,
) -> Result<engine::EngineChoices, UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>()
            .writing(|| engine::save_choice(engine, &model))
    })
    .await?
}

/// Save `transcription.language` into config.jsonc: `auto`, or the whisper
/// code people speak, such as `mr`. Takes effect on the next recording.
#[tauri::command]
#[specta::specta]
pub async fn set_spoken_language(
    app: AppHandle,
    language: String,
) -> Result<engine::EngineChoices, UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>()
            .writing(|| engine::save_language(&language))
    })
    .await?
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

/// Pause the live recording (TUR-146): nothing is written or transcribed
/// until [`resume_recording`], and the timer stops. Instant: the ticker
/// thread stops the audio at its next tick. Not a recording: no change.
#[tauri::command]
#[specta::specta]
pub fn pause_recording(app: AppHandle, recorder: State<'_, Recorder>) -> Status {
    recorder.pause(&app)
}

/// Carry on recording into the same meeting after [`pause_recording`].
#[tauri::command]
#[specta::specta]
pub fn resume_recording(app: AppHandle, recorder: State<'_, Recorder>) -> Status {
    recorder.resume(&app)
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
        let made = app
            .state::<FolderGate>()
            .writing(|| tickets::create(&title, &body));
        // TUR-113: sent to the tracker on its own, when one is set up.
        if let Ok(made) = &made {
            crate::sync::auto::queue(&app, &[made.id.as_str()]);
        }
        made
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
