//! The IPC surface: every `invoke` the webview can make, in one place.
//!
//! These are thin on purpose. Each one is an argument check, a call into a
//! module beside this one, and a conversion into [`UiError`]. Logic that lives
//! here instead of in a module is logic with no unit test, because a
//! `#[tauri::command]` needs an `AppHandle` to call.
//!
//! The matching TypeScript wrappers are in `src/ipc/client.ts`; the two files
//! are a pair and should be edited together.

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt as _;

use crate::engine::{self, Downloads, EnvironmentView, ModelView, SelectionView};
use crate::error::UiError;
use crate::meetings::{self, MeetingDetail, MeetingList};
use crate::onboarding;
use crate::permission;
use crate::recording::{Recorder, Status};

// --- meetings ------------------------------------------------------------

#[tauri::command]
pub fn list_meetings() -> Result<MeetingList, UiError> {
    meetings::list()
}

#[tauri::command]
pub fn read_meeting(id: String) -> Result<MeetingDetail, UiError> {
    meetings::detail(&id)
}

#[tauri::command]
pub fn save_notes(id: String, body: String) -> Result<(), UiError> {
    meetings::write_notes(&id, &body)
}

/// Open a meeting's folder in Finder.
///
/// L7 makes the files the product, so "where is it on disk" is a first-class
/// question rather than a debugging affordance.
#[tauri::command]
pub fn reveal_meeting(app: AppHandle, id: String) -> Result<(), UiError> {
    let detail = meetings::detail(&id)?;
    app.opener()
        .open_path(&detail.path, None::<&str>)
        .map_err(|error| UiError::app("open-failed", error.to_string()))
}

// --- permission and onboarding -------------------------------------------

#[tauri::command]
pub fn permission_status() -> permission::Status {
    permission::status()
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

#[tauri::command]
pub fn onboarding_state() -> Result<onboarding::State, UiError> {
    onboarding::state()
}

#[tauri::command]
pub fn complete_onboarding() -> Result<onboarding::State, UiError> {
    onboarding::complete()
}

#[tauri::command]
pub fn reset_onboarding() -> Result<onboarding::State, UiError> {
    onboarding::reset()
}

// --- engine and models ----------------------------------------------------

/// Filesystem-only, sub-millisecond. The settings route blocks on this.
#[tauri::command]
pub fn engine_environment() -> EnvironmentView {
    engine::environment(engine::DEFAULT_LOCALE, engine::DEFAULT_MODEL)
}

/// Runs `meet-stt --probe`, median ~160 ms. The settings route renders a
/// "Checking…" row and calls this after paint.
#[tauri::command]
pub async fn engine_selection() -> Result<SelectionView, UiError> {
    // The probe spawns a process and waits on it, which would otherwise park a
    // tokio worker thread for the whole 160 ms.
    tauri::async_runtime::spawn_blocking(|| {
        engine::resolve(
            stt::registry::Preference::Auto,
            engine::DEFAULT_LOCALE,
            engine::DEFAULT_MODEL,
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
/// The claim/release pair is here rather than inside `engine::download` so the
/// managed `Downloads` state never has to cross into the `'static` task that
/// does the work. One in-flight download per model: two would resume the same
/// `.part` file from two directions and race the atomic rename.
#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    downloads: State<'_, Downloads>,
    id: String,
) -> Result<String, UiError> {
    if !downloads.claim(&id) {
        return Err(UiError::app(
            "download-already-running",
            "That model is already downloading.",
        ));
    }
    let result = engine::download(app, id.clone()).await;
    downloads.release(&id);
    result
}

// --- recording ------------------------------------------------------------

#[tauri::command]
pub fn recording_status(recorder: State<'_, Recorder>) -> Status {
    recorder.status()
}

#[tauri::command]
pub fn toggle_recording(app: AppHandle, recorder: State<'_, Recorder>) -> Result<Status, UiError> {
    recorder.toggle(&app)
}

#[tauri::command]
pub fn stop_recording(app: AppHandle, recorder: State<'_, Recorder>) -> Result<Status, UiError> {
    recorder.stop(&app)
}
