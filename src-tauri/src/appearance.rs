//! Light, Dark or System, and the see-through glass switch (TUR-102): the
//! `appearance` section of `config.jsonc`, read and saved for the Settings
//! screen.
//!
//! The webview applies both itself (`src/lib/appearance.ts` sets
//! `data-theme` and `data-glass` on `<html>`). This side does the one thing
//! the webview cannot: it sets the app's native appearance, so the window
//! material behind the see-through sidebar, the title bar and the traffic
//! lights match the colours drawn over them. With `"system"` the app follows
//! the OS again. Tauri's `set_theme` is cross-platform; where an OS has no
//! such switch it does nothing, and the webview's own colours still apply.

use tauri::AppHandle;

use crate::config::{self, AppearanceConfig, Theme};
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::FolderGate;

/// The native appearance for a saved theme. `None` follows the OS.
fn native_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    }
}

/// Give the app the native appearance `theme` asks for.
fn apply_native(app: &AppHandle, theme: Theme) {
    app.set_theme(native_theme(theme));
}

/// At launch: the saved theme, before the window shows its first frame.
pub fn init(app: &AppHandle) {
    apply_native(app, config::appearance().theme);
}

/// `appearance` from `config.jsonc`, defaults when missing or not valid.
#[tauri::command]
#[specta::specta]
pub async fn appearance_settings() -> Result<AppearanceConfig, UiError> {
    on_blocking_pool(config::appearance).await
}

/// Save the theme and the glass switch, and return them as saved. Writes
/// under the meetings root, so through the [`FolderGate`]. The native
/// appearance changes at once.
#[tauri::command]
#[specta::specta]
pub async fn set_appearance(
    app: AppHandle,
    appearance: AppearanceConfig,
) -> Result<AppearanceConfig, UiError> {
    let handle = app.clone();
    let saved = on_blocking_pool(move || {
        use tauri::Manager as _;
        handle
            .state::<FolderGate>()
            .writing(|| Ok(config::set_appearance(appearance)?))
    })
    .await??;
    apply_native(&app, saved.theme);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_follows_the_os_and_the_others_force_their_look() {
        assert_eq!(native_theme(Theme::System), None);
        assert_eq!(native_theme(Theme::Light), Some(tauri::Theme::Light));
        assert_eq!(native_theme(Theme::Dark), Some(tauri::Theme::Dark));
    }
}
