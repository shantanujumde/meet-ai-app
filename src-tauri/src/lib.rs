//! The meet-ai desktop app.
//!
//! Phase 2 (SPEC §5): the app shell. Plugin wiring, the IPC surface in
//! [`commands`], the recording state machine, the ⌘⇧R global shortcut, and
//! the live transcript that runs alongside a recording ([`live_transcript`]).

mod agent_run;
mod agent_setup;
// TUR-58: "Start at login".
mod autostart;
mod bindings;
mod brief;
mod calendar;
// TUR-58: `meet-ai --toggle-recording`.
mod cli;
mod commands;
mod config;
mod copy_prompt;
mod detection;
mod engine;
mod error;
mod events;
mod folder_move;
// TUR-63: the user's own commands at three moments of a meeting.
mod hooks;
// TUR-76: closing the window hides it; quitting asks first while recording.
mod lifecycle;
mod live_transcript;
mod lock;
mod logs;
mod meetings;
mod mic_setting;
mod notify;
mod onboarding;
mod permission;
mod platform;
mod recording;
mod recording_state;
mod retention;
mod search;
mod settings_links;
// TUR-58: the record shortcut per OS.
mod shortcut;
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
const RECORD_SHORTCUT: &str = platform::RECORD_SHORTCUT;

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
    // TUR-46: where the log and crash files go this launch; `None` before
    // onboarding, which keeps them in the OS log folder.
    let logs_dir = logs::meetings_logs_dir();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // Two recorders would fight over the system audio tap, so a second
        // launch must focus the existing window rather than start a new app.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::Manager as _;
            // TUR-58: `meet-ai --toggle-recording` (Wayland's stand-in for
            // the shortcut) toggles in this, the running app, and shows no
            // window, the same as pressing the shortcut.
            if cli::wants_toggle(&argv) {
                spawn_toggle(app, cli::TOGGLE_RECORDING_FLAG);
                return;
            }
            // TUR-76: the window may be hidden, with no Dock icon.
            lifecycle::show_main_window(app);
            if let Some(window) = app.webview_windows().values().next() {
                let _ = window.set_focus();
            }
        }));
        builder = builder.plugin(global_shortcut_plugin());
        // TUR-58: "Start at login", off until the Settings switch says so.
        builder = builder.plugin(autostart::plugin());
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        // TUR-46: `.app/logs/meet-ai.log`, size-capped (see `logs::plugin`).
        .plugin(logs::plugin(logs_dir.clone()))
        .manage(logs::LogsDir(logs_dir))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .manage(recording::Recorder::default())
        .manage(live_transcript::LiveTranscript::default())
        .manage(engine::Downloads::default())
        .manage(folder_move::FolderGate::default())
        .manage(watch::MeetingsWatch::default())
        .manage(search::SearchIndex::default())
        .manage(agent_run::AgentRuns::default())
        .manage(sync::SyncRuns::default())
        .manage(detection::Detection::default())
        .manage(detection::popup::PromptPopup::default())
        .manage(calendar::CalendarState::default())
        // TUR-44: Google and Microsoft sign-in; TUR-47/48 read access tokens from it.
        .manage(calendar::signin::auth())
        .manage(retention::AudioRetention::default())
        // TUR-76: closing the main window hides it rather than quitting.
        .manage(lifecycle::Lifecycle::default())
        // TUR-77: the menu bar's Record names the meeting from the event clicked.
        .manage(recording::auto_title::PinnedEvent::default())
        .on_window_event(lifecycle::on_window_event)
        .setup(|_app| {
            // TUR-46: first, so a panic anywhere below leaves a crash file.
            // TUR-90: and the OS log folder for it once the meetings one moves.
            logs::set_crash_fallback(_app.handle());
            if let Some(dir) = logs::resolve(_app.handle()) {
                logs::install_crash_handlers(dir);
            }
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
            // TUR-47/48: the cloud calendars read their sign-in through the app handle.
            calendar::cloud::init(_app.handle());
            // TUR-27: watch for a meeting app opening, and ask before recording.
            let detection_config = crate::config::detection();
            detection::start(_app.handle(), detection_config.processes);
            // TUR-31: and for the mic and speakers both in use, like a call in a browser.
            detection::start_audio_activity(_app.handle(), detection_config.audio_activity);
            // TUR-30: and remind a minute before each meeting on the calendar.
            detection::start_reminders(_app.handle(), detection_config.calendar);
            // TUR-100: notice edits made to the meetings folder outside the app.
            watch::state(_app.handle()).restart(_app.handle());
            // TUR-101: open the search index now, rebuilding it if it is missing.
            {
                let handle = _app.handle().clone();
                std::thread::spawn(move || search::state(&handle).warm());
            }
            // TUR-45: delete audio older than `audio.retention_days`, soon
            // after launch and then daily.
            retention::start(_app.handle());
            // TUR-63: run the user's hooks when a meeting ends and its notes are written.
            hooks::app::init(_app.handle());
            // TUR-58: the first instance never records from its flag (SPEC
            // L15, `cli.rs`); it only says so in the log.
            if cli::wants_toggle(&std::env::args().collect::<Vec<_>>()) {
                tracing::info!("started with --toggle-recording and no app running: not recording");
            }
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                register_record_shortcut(_app.handle());
                tray::init(_app.handle());
                // TUR-76: after the tray, which decides whether close can hide.
                lifecycle::init(_app.handle());
            }
            Ok(())
        })
        .invoke_handler(bindings::builder().invoke_handler())
        .build(tauri::generate_context!())
        .expect("meet-ai failed to start")
        .run(|app, event| {
            // TUR-76: hold ⌘Q / the menu-bar Quit while recording, and
            // reopen the hidden window on a Dock click.
            lifecycle::on_run_event(app, &event);
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
