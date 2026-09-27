//! The meet-ai desktop app.
//!
//! Phase 2a (SPEC §5): the app shell. Plugin wiring, the IPC surface in
//! [`commands`], the recording state machine, and the ⌘⇧R global shortcut.
//! The live transcript pane is deliberately absent — it belongs to Phase 2b and
//! waits on the streaming session API.

// The `log` facade, re-exported by tauri-plugin-log. The Rust crates use
// `tracing`; only the plugin's own level filters need `log` types.
use tauri_plugin_log::log;

mod commands;
mod engine;
mod error;
mod meetings;
mod onboarding;
mod permission;
mod recording;
// The menu bar is a desktop surface; the mobile targets have nothing to put an
// item in.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod tray;

/// Start/stop recording from anywhere, including with the window unfocused.
///
/// SPEC §5 Phase 2 names ⌘⇧R specifically. It is registered in Rust rather than
/// in the webview because a webview key handler only fires while the window has
/// focus, and the whole point is that the user is in Zoom when they press it.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const RECORD_SHORTCUT: &str = "CmdOrCtrl+Shift+R";

/// Start the app.
///
/// # Panics
///
/// Panics if Tauri cannot build the app context — that means the bundled
/// configuration is broken, which is a build-time mistake, not a runtime state
/// the user can do anything about.
pub fn run() {
    // Logging goes through tauri-plugin-log so frontend and Rust lines land in
    // the same file (SPEC §2.2).
    let mut builder = tauri::Builder::default();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // Two recorders would fight over the system audio tap, so a second
        // launch must focus the existing window rather than start a new app.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager as _;
            if let Some(window) = app.webview_windows().values().next() {
                let _ = window.set_focus();
            }
        }));
        builder = builder.plugin(global_shortcut_plugin());
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                // tao and wry log every AppKit callback at TRACE. Left alone
                // they bury our own lines under thousands of theirs, which
                // makes a user-submitted log file useless.
                .level_for("tao", log::LevelFilter::Warn)
                .level_for("wry", log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(recording::Recorder::default())
        .manage(engine::Downloads::default())
        .setup(|_app| {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                register_record_shortcut(_app.handle());
                tray::init(_app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_meetings,
            commands::read_meeting,
            commands::save_notes,
            commands::reveal_meeting,
            commands::permission_status,
            commands::open_privacy_settings,
            commands::onboarding_state,
            commands::complete_onboarding,
            commands::reset_onboarding,
            commands::engine_environment,
            commands::engine_selection,
            commands::model_catalogue,
            commands::download_model,
            commands::recording_status,
            commands::toggle_recording,
            commands::stop_recording,
        ])
        .run(tauri::generate_context!())
        .expect("meet-ai failed to start");
}

/// The global-shortcut plugin, with the ⌘⇧R handler attached.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
// Not generic over `Runtime`: the plugin builder is bound to Wry, and the app
// below runs on Wry. Making this generic only moves the mismatch.
fn global_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri::Manager as _;
    use tauri_plugin_global_shortcut::ShortcutState;

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            // A single keypress delivers Pressed *and* Released. Without this
            // filter ⌘⇧R starts a recording and immediately stops it again,
            // which looks exactly like the shortcut not working at all.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let Some(recorder) = app.try_state::<recording::Recorder>() else {
                tracing::error!("the recorder state is missing; ignoring the record shortcut");
                return;
            };
            match recorder.toggle(app) {
                Ok(status) => tracing::info!(phase = ?status.phase, "record shortcut"),
                Err(error) => {
                    // The window may be closed or unfocused — that is the whole
                    // point of a global shortcut — so there may be nothing on
                    // screen to put an error next to. A notification is the one
                    // surface that is guaranteed to be visible.
                    tracing::warn!(message = %error.message, "record shortcut refused");
                    notify_refusal(app, &error.message);
                }
            }
        })
        .build()
}

/// Register ⌘⇧R once the app is up.
///
/// A failure here is not fatal: every other way to start a recording still
/// works, and taking the app down because another app already owns the
/// shortcut would be a wildly disproportionate response.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn register_record_shortcut<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt as _;

    match app.global_shortcut().register(RECORD_SHORTCUT) {
        Ok(()) => tracing::info!(shortcut = RECORD_SHORTCUT, "recording shortcut registered"),
        Err(error) => tracing::warn!(
            %error,
            shortcut = RECORD_SHORTCUT,
            "could not register the recording shortcut; another app may already own it"
        ),
    }
}

/// Tell the user why a shortcut press did nothing, on a surface that does not
/// need the window to be open.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn notify_refusal<R: tauri::Runtime>(app: &tauri::AppHandle<R>, message: &str) {
    use tauri_plugin_notification::NotificationExt as _;

    if let Err(error) = app
        .notification()
        .builder()
        .title("meet-ai did not start recording")
        .body(message)
        .show()
    {
        tracing::warn!(%error, "could not show the refusal notification either");
    }
}
