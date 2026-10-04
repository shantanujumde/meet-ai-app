//! "Use the Mac's own mic when Bluetooth headphones are connected" (TUR-91):
//! `audio.use_builtin_mic_with_bluetooth` in `config.jsonc`, on by default.
//!
//! Opening a Bluetooth headset's mic switches it to call mode, which makes
//! everything the user hears quieter. With this on, `audio::mic` records the
//! Mac's built-in mic instead whenever the default input is Bluetooth. The
//! audio crate holds the value; [`apply`] copies it there from the file
//! before every permission check (and so before every recording), so a hand
//! edit needs no restart.

use tauri::{AppHandle, Manager as _};

use crate::config;
use crate::error::UiError;
use crate::folder_move::FolderGate;

/// Read the setting from `config.jsonc` and hand it to the audio crate.
pub fn apply() {
    audio::mic_choice::set_use_builtin_with_bluetooth(config::use_builtin_mic_with_bluetooth());
}

/// The setting as saved.
#[tauri::command]
#[specta::specta]
pub async fn builtin_mic_with_bluetooth() -> Result<bool, UiError> {
    on_blocking_pool(config::use_builtin_mic_with_bluetooth).await
}

/// Save the setting and return it as saved. Writes under the meetings root,
/// so through the [`FolderGate`]. The next mic open uses it.
#[tauri::command]
#[specta::specta]
pub async fn set_builtin_mic_with_bluetooth(app: AppHandle, on: bool) -> Result<bool, UiError> {
    let saved = on_blocking_pool(move || {
        app.state::<FolderGate>()
            .writing(|| Ok(config::set_use_builtin_mic_with_bluetooth(on)?))
    })
    .await??;
    audio::mic_choice::set_use_builtin_with_bluetooth(saved);
    Ok(saved)
}

async fn on_blocking_pool<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, UiError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))
}
