//! The meet-ai desktop app.
//!
//! Phase 2 (SPEC §5): the app shell. Plugin wiring, the IPC surface in
//! [`commands`], the recording state machine, the ⌘⇧R global shortcut, and
//! the live transcript that runs alongside a recording ([`live_transcript`]).

mod agent_run;
mod agent_setup;
// TUR-158: the app command list `build.rs` declares; here for its tests.
#[cfg(test)]
mod app_commands;
// TUR-102: Light / Dark / System and the glass switch.
mod appearance;
// TUR-58: "Start at login".
mod autostart;
mod bindings;
mod brief;
mod calendar;
// TUR-160: agent runs by key, so quitting can stop them all.
mod cancels;
// TUR-58: `meet-ai --toggle-recording`.
mod cli;
mod commands;
mod config;
// TUR-155: "this setting in config.jsonc was not valid", for the Settings cards.
mod config_problem;
mod copy_prompt;
mod detection;
mod engine;
mod error;
mod events;
mod folder_move;
// TUR-65: "No headphones" while recording through speakers.
mod headphone_warning;
// TUR-63: the user's own commands at three moments of a meeting.
mod hooks;
mod ipc_defaults;
// TUR-76: closing the window hides it; quitting asks first while recording.
mod lifecycle;
mod live_transcript;
mod lock;
mod logs;
mod meetings;
mod mic_setting;
mod notify;
mod onboarding;
// TUR-146: the small always-on-top window while recording.
mod overlay;
mod permission;
mod platform;
mod recording;
mod recording_state;
mod retention;
mod search;
mod settings_links;
// TUR-171: every saved setting Settings shows, from one config read.
mod settings_snapshot;
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
    // TUR-125: crash handlers first, so a crash before `setup` leaves a file.
    // Before onboarding (`None`) such a crash only gets the OS crash report.
    if let Some(dir) = logs_dir.clone() {
        logs::install_early(dir);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        // Two recorders would fight over the system audio tap, so a second
        // launch must focus the existing window rather than start a new app.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // TUR-58: `meet-ai --toggle-recording` (Wayland's stand-in for
            // the shortcut) toggles in this, the running app, and shows no
            // window, the same as pressing the shortcut.
            if cli::wants_toggle(&argv) {
                spawn_toggle(app, cli::TOGGLE_RECORDING_FLAG);
                return;
            }
            // TUR-76: the window may be hidden, with no Dock icon.
            // `show_main_window` focuses the main window by its label; the
            // first window in the map can be the prompt card or the hidden
            // overlay instead (TUR-158).
            lifecycle::show_main_window(app);
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
        // TUR-160: the agent Test runs, so quitting stops them.
        .manage(agent_setup::TestRuns::default())
        // TUR-113: tickets go to the tracker on their own, one at a time.
        .manage(sync::auto::AutoSync::default())
        .manage(detection::Detection::default())
        .manage(detection::popup::PromptPopup::default())
        .manage(calendar::CalendarState::default())
        // TUR-44: Google and Microsoft sign-in; TUR-47/48 read access tokens from it.
        .manage(calendar::signin::auth())
        .manage(calendar::cancel::SignInCancels::default())
        .manage(retention::AudioRetention::default())
        // TUR-76: closing the main window hides it rather than quitting.
        .manage(lifecycle::Lifecycle::default())
        // TUR-146: wakes the overlay worker.
        .manage(overlay::Overlay::default())
        // TUR-169: whether the record shortcut is ours, for the window.
        .manage(shortcut::RecordShortcut::default())
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
            // TUR-102: the saved Light / Dark / System, before the window paints.
            appearance::init(_app.handle());
            // TUR-47/48: the cloud calendars read their sign-in through the app handle.
            calendar::cloud::init(_app.handle());
            // TUR-27: watch for a meeting app opening, and ask before recording.
            let detection_config = crate::config::detection();
            detection::start(_app.handle(), detection_config.processes);
            // TUR-31: and for the mic and speakers both in use, like a call in a browser.
            detection::start_audio_activity(_app.handle(), detection_config.audio_activity);
            // TUR-143: and for a call app or a browser using the mic, by name.
            detection::call_start::start(_app.handle());
            // TUR-30: and remind a minute before each meeting on the calendar.
            detection::start_reminders(_app.handle(), detection_config.calendar);
            // TUR-144: and for the call app hanging up while recording.
            detection::call_end::start(_app.handle());
            // TUR-100: notice edits made to the meetings folder outside the app.
            watch::state(_app.handle()).restart(_app.handle());
            // TUR-113: tickets synced before suggestions existed move to Tickets.
            tickets::start_migration(_app.handle());
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
            // TUR-65: warn while recording through speakers (SPEC L6).
            headphone_warning::init(_app.handle());
            // TUR-145: the computer going to sleep stops and saves the recording.
            recording::backup_stop::install_sleep_stop(_app.handle());
            // TUR-146: the recording overlay follows the recorder.
            overlay::init(_app.handle());
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
            // TUR-97/TUR-160: `Exit` is the last event before the process
            // ends. Stop every agent CLI and finish the recording in whatever
            // phase it is, bounded; a held quit has already done it.
            if let tauri::RunEvent::Exit = event {
                lifecycle::quit::on_exit(app);
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
    use tauri::Manager as _;
    use tauri_plugin_global_shortcut::GlobalShortcutExt as _;

    let registered = app.global_shortcut().register(RECORD_SHORTCUT);
    // TUR-169: the window says the shortcut is unavailable instead of
    // advertising one that does nothing.
    if let Some(state) = app.try_state::<shortcut::RecordShortcut>() {
        state.registered(registered.is_ok());
    }
    match registered {
        Ok(()) => tracing::info!(shortcut = RECORD_SHORTCUT, "recording shortcut registered"),
        Err(error) => tracing::warn!(
            %error,
            shortcut = RECORD_SHORTCUT,
            "could not register the recording shortcut; another app may already own it"
        ),
    }
}
