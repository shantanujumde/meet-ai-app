//! "Start at login" (TUR-58), through `tauri-plugin-autostart`: a LaunchAgent
//! on macOS, the `Run` registry key on Windows, an XDG autostart entry on
//! Linux. Off by default: the plugin never turns itself on, and only the
//! Settings switch calls [`set_start_at_login`].
//!
//! The OS holds the setting, not `config.jsonc`: the login item is the truth,
//! and a copy in config could only drift from it.
//!
//! A login launch is a normal launch: it never records (SPEC L15).

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as _;

use crate::error::UiError;

/// The plugin, ready for `Builder::plugin`.
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None)
}

/// Whether meet-ai starts when the user logs in.
#[tauri::command]
#[specta::specta]
pub async fn start_at_login(app: AppHandle) -> Result<bool, UiError> {
    tauri::async_runtime::spawn_blocking(move || app.autolaunch().is_enabled())
        .await
        .map_err(|error| UiError::app("task-failed", error.to_string()))?
        .map_err(|error| UiError::app("autostart", error.to_string()))
}

/// Turn "Start at login" on or off, and return what the OS now says.
#[tauri::command]
#[specta::specta]
pub async fn set_start_at_login(app: AppHandle, enabled: bool) -> Result<bool, UiError> {
    tauri::async_runtime::spawn_blocking(move || {
        let launcher = app.autolaunch();
        let result = if enabled {
            launcher.enable()
        } else {
            launcher.disable()
        };
        result.and_then(|()| launcher.is_enabled())
    })
    .await
    .map_err(|error| UiError::app("task-failed", error.to_string()))?
    .map_err(|error| UiError::app("autostart", error.to_string()))
}
