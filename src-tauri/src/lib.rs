//! The meet-ai desktop app.
//!
//! Scaffold only. Commands, events, tray and the recording state machine land in
//! Phase 2 (SPEC §5). What is here is the plugin wiring, so that adding a
//! command is a one-line change rather than a plugin archaeology exercise.

// The `log` facade, re-exported by tauri-plugin-log. The Rust crates use
// `tracing`; only the plugin's own level filters need `log` types.
use tauri_plugin_log::log;

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
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
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
        .run(tauri::generate_context!())
        .expect("meet-ai failed to start");
}
