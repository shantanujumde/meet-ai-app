//! Closing the window is not quitting (TUR-76).
//!
//! meet-ai keeps running in the menu bar after its main window is closed, like
//! Granola, so the calendar reminders (TUR-30), meeting detection (TUR-27/31),
//! the Today refresh and a recording in progress all carry on.
//!
//! - **Close** (red button, ⌘W, Alt+F4) hides the window. On macOS the Dock
//!   icon goes with it (accessory mode) unless `app.show_in_dock_when_closed`
//!   is set. The first close posts a one-off notice ([`notice`]).
//! - **Quit** is the menu-bar "Quit meet-ai", ⌘Q / the app menu's Quit, or the
//!   OS. The first two reach `RunEvent::ExitRequested`, where a quit while
//!   recording is held and the window asks "Stop recording and quit?"
//!   ([`QUIT_CONFIRM_EVENT`]); "Stop and quit" comes back as [`confirm_quit`].
//!   Logout and shutdown go straight to `RunEvent::Exit`, which stops the
//!   recording through the normal stop path (`lib.rs`) without asking.
//! - **Reopen** is the menu-bar "Open meet-ai", a Dock click (`Reopen`), a
//!   second launch (single-instance), all [`show_main_window`], and a new
//!   detection or reminder prompt ([`reveal_for_prompt`]).
//!
//! The decisions are the pure functions at the top, unit-tested; the Tauri
//! wiring below them stays thin.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _, RunEvent, Window, WindowEvent};

use crate::config::{self, AppConfig};
use crate::error::{UiError, on_blocking_pool};
use crate::events::NAVIGATE_EVENT;
use crate::events::QUIT_CONFIRM_EVENT;
use crate::folder_move::FolderGate;
use crate::platform;
use crate::recording::{Phase, Recorder};

pub(crate) mod notice;

/// The app menu's own Quit item (macOS), in place of the stock one.
const QUIT_MENU_ITEM: &str = "app-quit";
/// The menu-bar item's id, as `tray.rs` builds it.
const TRAY_ID: &str = "meet-ai";
/// Tauri's label for the one window in `tauri.conf.json`.
const MAIN_WINDOW: &str = "main";

// --- decisions --------------------------------------------------------------

/// What closing the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide it and keep running in the menu bar or tray.
    Hide,
    /// Quit, through the same path as the Quit item.
    Quit,
}

/// What [`on_close_requested`] needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseState {
    /// There is a menu-bar or tray item to bring the window back from.
    pub has_tray: bool,
}

/// Closing hides, as long as there is a way back. Linux with no tray (GNOME
/// without AppIndicator support, where `tray-icon` cannot build one) has
/// none, so there closing quits — a running app with no window and no icon
/// could only be ended from a terminal.
pub fn on_close_requested(state: CloseState) -> CloseAction {
    if state.has_tray {
        CloseAction::Hide
    } else {
        CloseAction::Quit
    }
}

/// What a quit request does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitAction {
    /// Let the app end. A recording is stopped on the way out (`lib.rs`).
    Quit,
    /// Hold the quit and ask "Stop recording and quit?" in the window.
    AskFirst,
}

/// Ask first while anything is recording, so a meeting is never cut by a
/// stray ⌘Q.
pub fn on_quit_requested(recording: bool) -> QuitAction {
    if recording {
        QuitAction::AskFirst
    } else {
        QuitAction::Quit
    }
}

/// What [`on_exit_requested`] needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitRequest {
    /// The updater is restarting the app. Tauri ignores a prevent for that
    /// anyway, so it is never asked about.
    pub restart: bool,
    /// The user already answered "Stop and quit".
    pub confirmed: bool,
    /// A recording is starting, running or stopping.
    pub recording: bool,
}

/// The decision at `RunEvent::ExitRequested`, which every quit but the OS's
/// passes: the menu-bar Quit, ⌘Q, and "Stop and quit" itself.
pub fn on_exit_requested(request: ExitRequest) -> QuitAction {
    if request.restart || request.confirmed {
        QuitAction::Quit
    } else {
        on_quit_requested(request.recording)
    }
}

/// Starting and stopping count as recording: quitting mid-way through either
/// would cut the files as surely as quitting mid-meeting.
pub fn is_recording(phase: Phase) -> bool {
    phase != Phase::Idle
}

/// Whether the Dock icon stays while the window is hidden (macOS).
pub fn dock_visible_when_hidden(config: AppConfig) -> bool {
    config.show_in_dock_when_closed
}

// --- state ------------------------------------------------------------------

/// What the quit path remembers between the question and the answer.
#[derive(Debug, Default)]
pub struct Lifecycle {
    /// "Stop and quit" was clicked; the next exit request goes through.
    quit_confirmed: AtomicBool,
}

// --- wiring -----------------------------------------------------------------

/// Called once from `setup`: the asking ⌘Q.
pub fn init(app: &AppHandle) {
    platform::install_quit_menu(app, QUIT_MENU_ITEM, request_quit);
}

/// A new reminder or detection prompt (TUR-27/30) was just delivered. Its
/// Record / Dismiss / Open brief live in the window, so a hidden window comes
/// back, without taking focus from the call the user is in. TUR-59 replaces
/// this with a small popup of its own.
pub fn reveal_for_prompt(app: &AppHandle) {
    if !main_window_visible(app) {
        reveal_main_window(app, false);
    }
}

/// `Builder::on_window_event`: a close of the main window hides it.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    // TUR-58: the Windows taskbar may have switched light/dark.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let WindowEvent::ThemeChanged(_) = event {
        crate::tray::refresh_icon(window.app_handle());
    }
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != MAIN_WINDOW {
        return;
    }
    let app = window.app_handle();
    api.prevent_close();
    match on_close_requested(CloseState {
        has_tray: platform::has_tray(app, TRAY_ID),
    }) {
        CloseAction::Hide => hide_main_window(app),
        CloseAction::Quit => request_quit(app),
    }
}

/// `App::run`'s callback: hold a quit while recording, and reopen on a Dock
/// click.
pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    if let RunEvent::ExitRequested { code, api, .. } = event {
        let request = ExitRequest {
            restart: *code == Some(tauri::RESTART_EXIT_CODE),
            confirmed: app
                .try_state::<Lifecycle>()
                .is_some_and(|state| state.quit_confirmed.load(Ordering::SeqCst)),
            recording: app
                .try_state::<Recorder>()
                .is_some_and(|recorder| is_recording(recorder.status().phase)),
        };
        if on_exit_requested(request) == QuitAction::AskFirst {
            api.prevent_exit();
            ask_before_quitting(app);
        }
    } else if platform::is_reopen(event) {
        show_main_window(app);
    }
}

/// Quit the way the menu-bar item does: through `ExitRequested`, which asks
/// first while recording.
pub fn request_quit(app: &AppHandle) {
    app.exit(0);
}

/// Bring the main window back and in front, with its Dock icon.
pub fn show_main_window(app: &AppHandle) {
    reveal_main_window(app, true);
}

fn reveal_main_window(app: &AppHandle, focus: bool) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        tracing::warn!("no main window to show");
        return;
    };
    platform::set_dock_visible(app, true);
    let _ = window.show();
    let _ = window.unminimize();
    if focus {
        let _ = window.set_focus();
    }
}

fn main_window_visible(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN_WINDOW)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW)
        && let Err(error) = window.hide()
    {
        tracing::warn!(%error, "could not hide the main window");
        return;
    }
    if !dock_visible_when_hidden(config::app()) {
        platform::set_dock_visible(app, false);
    }
    tracing::info!("main window closed; still running in the menu bar");
    // Off the main thread: the flag is a file write under the meetings root.
    let handle = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("meet-ai-still-running".to_string())
        .spawn(move || notice::show_once(&handle))
    {
        tracing::warn!(%error, "could not spawn the still-running notice");
    }
}

/// A screen Rust sends the window to, on [`NAVIGATE_EVENT`] (TUR-77).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "to", rename_all = "camelCase")]
pub enum NavigateTo {
    /// The pre-meeting brief for the meeting called `title` (TUR-32).
    Brief { title: String },
    /// Settings, where the calendar is connected.
    Settings,
}

/// Bring the main window back and send it to `to`.
pub fn navigate(app: &AppHandle, to: NavigateTo) {
    show_main_window(app);
    if let Err(error) = app.emit(NAVIGATE_EVENT, &to) {
        tracing::warn!(%error, "could not send the window to a screen");
    }
}

/// Show the window and ask "Stop recording and quit?" there.
fn ask_before_quitting(app: &AppHandle) {
    tracing::info!("quit while recording; asking first");
    show_main_window(app);
    if let Err(error) = app.emit(QUIT_CONFIRM_EVENT, ()) {
        tracing::warn!(%error, "could not ask the window before quitting");
    }
}

// --- commands ---------------------------------------------------------------

/// The app settings the Settings screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// macOS: keep the Dock icon while the window is closed.
    pub show_in_dock_when_closed: bool,
}

impl From<AppConfig> for AppSettings {
    fn from(config: AppConfig) -> Self {
        Self {
            show_in_dock_when_closed: config.show_in_dock_when_closed,
        }
    }
}

/// "Stop and quit": let the held quit through. The exit hook in `lib.rs`
/// stops the recording the same way the Stop button does.
#[tauri::command]
#[specta::specta]
pub async fn confirm_quit(app: AppHandle) {
    if let Some(state) = app.try_state::<Lifecycle>() {
        state.quit_confirmed.store(true, Ordering::SeqCst);
    }
    tracing::info!("stop and quit confirmed");
    request_quit(&app);
}

/// `app` from `config.jsonc`, defaults when missing or not valid.
#[tauri::command]
#[specta::specta]
pub async fn app_settings() -> Result<AppSettings, UiError> {
    on_blocking_pool(|| AppSettings::from(config::app())).await
}

/// Save "Show in Dock when the window is closed" and return it as saved.
/// Writes under the meetings root, so through the [`FolderGate`].
#[tauri::command]
#[specta::specta]
pub async fn set_show_in_dock_when_closed(
    app: AppHandle,
    show: bool,
) -> Result<AppSettings, UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>().writing(|| {
            // TUR-77: the other `app` keys stay as they are on disk.
            let saved = config::set_app(|app| app.show_in_dock_when_closed = show)?;
            Ok(AppSettings::from(saved))
        })
    })
    .await?
}

/// `app.menu_bar_countdown` (TUR-77): whether the next meeting shows next to
/// the menu-bar icon.
#[tauri::command]
#[specta::specta]
pub async fn menu_bar_countdown() -> Result<bool, UiError> {
    on_blocking_pool(|| config::app().menu_bar_countdown).await
}

/// Save "Show next meeting in the menu bar" and return it as saved. The menu
/// bar picks it up at once rather than at its next minute.
#[tauri::command]
#[specta::specta]
pub async fn set_menu_bar_countdown(app: AppHandle, show: bool) -> Result<bool, UiError> {
    let handle = app.clone();
    let saved = on_blocking_pool(move || {
        handle.state::<FolderGate>().writing(|| {
            let saved = config::set_app(|app| app.menu_bar_countdown = show)?;
            Ok(saved.menu_bar_countdown)
        })
    })
    .await??;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    crate::tray::redraw_soon(&app);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_hides_when_there_is_a_tray_to_come_back_from() {
        assert_eq!(
            on_close_requested(CloseState { has_tray: true }),
            CloseAction::Hide
        );
    }

    #[test]
    fn closing_quits_when_there_is_no_way_back() {
        assert_eq!(
            on_close_requested(CloseState { has_tray: false }),
            CloseAction::Quit
        );
    }

    #[test]
    fn quitting_while_recording_asks_first() {
        assert_eq!(on_quit_requested(true), QuitAction::AskFirst);
        assert_eq!(on_quit_requested(false), QuitAction::Quit);
    }

    #[test]
    fn every_phase_but_idle_counts_as_recording() {
        assert!(!is_recording(Phase::Idle));
        for phase in [Phase::Starting, Phase::Recording, Phase::Stopping] {
            assert!(is_recording(phase), "{phase:?}");
        }
    }

    #[test]
    fn an_exit_request_while_recording_is_held_until_confirmed() {
        let asked = ExitRequest {
            restart: false,
            confirmed: false,
            recording: true,
        };
        assert_eq!(on_exit_requested(asked), QuitAction::AskFirst);
        assert_eq!(
            on_exit_requested(ExitRequest {
                confirmed: true,
                ..asked
            }),
            QuitAction::Quit,
            "Stop and quit goes through"
        );
    }

    #[test]
    fn an_exit_request_with_nothing_recording_quits_at_once() {
        assert_eq!(
            on_exit_requested(ExitRequest {
                restart: false,
                confirmed: false,
                recording: false,
            }),
            QuitAction::Quit
        );
    }

    #[test]
    fn an_updater_restart_is_never_held() {
        assert_eq!(
            on_exit_requested(ExitRequest {
                restart: true,
                confirmed: false,
                recording: true,
            }),
            QuitAction::Quit
        );
    }

    #[test]
    fn the_dock_icon_follows_the_setting() {
        assert!(!dock_visible_when_hidden(AppConfig::default()));
        assert!(dock_visible_when_hidden(AppConfig {
            show_in_dock_when_closed: true,
            ..AppConfig::default()
        }));
    }

    #[test]
    fn the_settings_reach_the_window_in_camel_case() {
        let settings = AppSettings::from(AppConfig {
            show_in_dock_when_closed: true,
            ..AppConfig::default()
        });
        assert_eq!(
            serde_json::to_value(settings).unwrap(),
            serde_json::json!({ "showInDockWhenClosed": true })
        );
    }

    #[test]
    fn a_navigation_reaches_the_window_tagged_by_screen() {
        assert_eq!(
            serde_json::to_value(NavigateTo::Brief {
                title: "Weekly sync".into()
            })
            .unwrap(),
            serde_json::json!({ "to": "brief", "title": "Weekly sync" })
        );
        assert_eq!(
            serde_json::to_value(NavigateTo::Settings).unwrap(),
            serde_json::json!({ "to": "settings" })
        );
    }
}
