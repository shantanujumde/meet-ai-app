//! The meet-ai desktop app.
//!
//! Phase 2 (SPEC §5): the app shell. Plugin wiring, the IPC surface in
//! [`commands`], the recording state machine, the ⌘⇧R global shortcut, and
//! the live transcript that runs alongside a recording ([`live_transcript`]).

// The `log` facade, re-exported by tauri-plugin-log. The Rust crates use
// `tracing`; only the plugin's own level filters need `log` types.
use tauri_plugin_log::log;

mod agent_run;
mod agent_setup;
mod bindings;
mod brief;
mod calendar;
mod commands;
mod config;
mod copy_prompt;
mod detection;
mod engine;
mod error;
mod events;
mod folder_move;
mod live_transcript;
mod lock;
mod meetings;
mod notify;
mod onboarding;
mod permission;
mod recording;
mod recording_state;
mod search;
mod sync;
mod tickets;
mod watch;
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
        .manage(live_transcript::LiveTranscript::default())
        .manage(engine::Downloads::default())
        .manage(folder_move::FolderGate::default())
        .manage(watch::MeetingsWatch::default())
        .manage(search::SearchIndex::default())
        .manage(agent_run::AgentRuns::default())
        .manage(sync::SyncRuns::default())
        .manage(detection::Detection::default())
        .manage(calendar::CalendarState::default())
        .setup(|_app| {
            // TUR-97: before the record shortcut exists, so nothing can be
            // mid-recording while this rewrites a header. Fast — two 44-byte
            // reads per meeting — and a no-op on every launch after the first
            // that finds something.
            {
                use tauri::Manager as _;
                let status = _app.state::<recording::Recorder>().status();
                let rewritten =
                    meetings::recover_interrupted_audio(meetings::Live::from_status(&status));
                if rewritten > 0 {
                    tracing::info!(rewritten, "made interrupted recordings' audio playable");
                }
            }
            // TUR-27: watch for a meeting app opening, and ask before recording.
            detection::start(_app.handle(), crate::config::detection().processes);
            // TUR-100: notice edits made to the meetings folder outside the app.
            watch::state(_app.handle()).restart(_app.handle());
            // TUR-101: open the search index now, rebuilding it if it is missing.
            {
                let handle = _app.handle().clone();
                std::thread::spawn(move || search::state(&handle).warm());
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                register_record_shortcut(_app.handle());
                tray::init(_app.handle());
            }
            Ok(())
        })
        .invoke_handler(bindings::builder().invoke_handler())
        .build(tauri::generate_context!())
        .expect("meet-ai failed to start")
        .run(|app, event| {
            // TUR-97: a normal quit mid-recording (⌘Q, the menu bar's Quit, a
            // logout asking apps to quit) used to leave the files exactly as a
            // `kill -9` does — up to one checkpoint of audio past the header
            // and the meeting labelled Interrupted. `Exit` is the last event
            // before the process ends, so stop the recording here the same way
            // the Stop button does. A no-op when nothing is recording; a hard
            // kill never reaches this, which is what the checkpoints are for.
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager as _;
                // TUR-10: first, so the stop below starts no notes run, and a
                // run already going has its agent stopped before it answers.
                agent_run::shutdown(app);
                if let Err(error) = app.state::<recording::Recorder>().stop(app) {
                    tracing::error!(message = %error.message, "could not finish the recording on quit");
                }
            }
        });
}

/// The global-shortcut plugin, with the ⌘⇧R handler attached.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
// Not generic over `Runtime`: the plugin builder is bound to Wry, and the app
// below runs on Wry. Making this generic only moves the mismatch.
fn global_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_global_shortcut::ShortcutState;

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            // A single keypress delivers Pressed *and* Released. Without this
            // filter ⌘⇧R starts a recording and immediately stops it again,
            // which looks exactly like the shortcut not working at all.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            spawn_toggle(app, "record shortcut");
        })
        .build()
}

/// Start or stop the recording from a native surface — ⌘⇧R or the menu-bar
/// item — without blocking the thread that delivered the event.
///
/// Both of those callbacks arrive on the main thread, the one AppKit draws the
/// window on. `Recorder::toggle` blocks on real wall-clock time (SPEC §8.1's
/// permission measurement and chime, then Core Audio opening or closing), so
/// calling it inline froze the whole app for however long that took — which is
/// exactly what the menu-bar item did until it shared this helper. A worker
/// thread keeps the callback itself instant. The phase-claiming mutex inside
/// `Recorder` is what makes a double-tap a no-op rather than a race, not the
/// timing of this call. The webview's button reaches the same `toggle` through
/// `commands::toggle_recording`, on a blocking-pool thread for the same reason.
///
/// `source` names the surface in the log, so a user-submitted log says which
/// control they actually used.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn spawn_toggle(app: &tauri::AppHandle, source: &'static str) {
    use tauri::Manager as _;

    if app.try_state::<recording::Recorder>().is_none() {
        tracing::error!(source, "the recorder state is missing; ignoring the toggle");
        return;
    }
    let app = app.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("meet-ai-record-toggle".to_string())
        .spawn(move || {
            // Which way this press was meant to go, for the notification's
            // title. Read before the toggle, because by the time it fails the
            // recorder is back at `Idle` either way.
            let stopping =
                app.state::<recording::Recorder>().status().phase == recording::Phase::Recording;
            // Through the gate, so a toggle cannot start a recording under the
            // old root while the meetings folder is moving.
            match folder_move::toggle_recording(&app) {
                Ok(status) => tracing::info!(source, phase = ?status.phase, "recording toggled"),
                Err(error) => {
                    // The window may be closed or unfocused — that is the whole
                    // point of a global shortcut and a menu-bar item — so there
                    // may be nothing on screen to put an error next to. A
                    // notification is the one surface guaranteed visible.
                    tracing::warn!(source, message = %error.message, "recording toggle refused");
                    notify::refusal(&app, stopping, &error);
                }
            }
        })
    {
        tracing::error!(source, %error, "could not spawn a worker thread for the recording toggle");
    }
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
