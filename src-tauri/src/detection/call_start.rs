//! A call starting in an app we can name (TUR-143): "WhatsApp call
//! detected", "Call detected in Google Chrome".
//!
//! Every [`detect::call_start::MIC_POLL_INTERVAL`] the loop here reads which
//! apps are using a mic (`audio::mic_users`, TUR-142) and hands the reading
//! to `detect`'s pure call-start rule (15 s hold, 10 min after Not now, the
//! built-in never-a-call list and the user's "Never detect" list, nothing
//! while recording). A call it reports goes through [`super::notify`], the one
//! prompt path, so the `detection.call_start` switch, "already recording" and
//! [`super::merge`] (a calendar reminder for the same call is one prompt)
//! apply as for every other prompt.
//!
//! Where the OS can list the apps on the mic, this replaces two older rules:
//!
//! - "a meeting app is open" (TUR-27) does not ask at all
//!   ([`process_switch`]); it stays the fallback for macOS older than 14,
//!   where the list cannot be read;
//! - "mic and speakers both in use" (TUR-31) asks only when an app we do not
//!   know is on the mic ([`Calls::audio_activity_allowed`]), so dictation or
//!   Krisp with music playing never asks, and a call app or a browser asks
//!   once, here, by name.
//!
//! "Never for <App>" on a prompt adds the app to `detection.never_detect`
//! ([`never_for`]); Not now starts the 10 minutes from the click
//! ([`Calls::dismissed`]). Nothing here records (L15).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Instant;

use audio::mic_users::{AppKind, MicApp, MicUsers};
use detect::{CallStarted, MicKind, MicLoop, MicLoopHooks, MicReading, MicSource, MicUser, Signal};
use tauri::{AppHandle, Manager as _};

use super::Detection;
use super::live::REREAD_AFTER;
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::FolderGate;
use crate::lock::lock_or_recover;

/// Managed with [`Detection`]: what the call-start loop knows, for the other
/// rules and for Settings.
pub struct Calls {
    /// Some reading so far listed the apps on the mic. Set from the real
    /// reads only, never from a switched-off one.
    supported: Arc<AtomicBool>,
    /// The first [`Self::list_supported`] asks the OS once, so the process
    /// loop does not ask "Zoom is open" before the first reading.
    probed: Once,
    /// The latest reading, for [`Self::audio_activity_allowed`].
    latest: Mutex<MicReading>,
    /// `detection.never_detect`, re-read at most every [`REREAD_AFTER`].
    never: Mutex<Option<(Vec<String>, Instant)>>,
    read_never: fn() -> Vec<String>,
    running: Mutex<Option<MicLoop>>,
}

impl Default for Calls {
    fn default() -> Self {
        Self::with_never_reader(crate::config::never_detect)
    }
}

impl Calls {
    /// Reading the "Never detect" list through `read`, for tests.
    fn with_never_reader(read: fn() -> Vec<String>) -> Self {
        Self {
            supported: Arc::new(AtomicBool::new(false)),
            probed: Once::new(),
            latest: Mutex::new(MicReading::NotSupported),
            never: Mutex::new(None),
            read_never: read,
            running: Mutex::new(None),
        }
    }

    /// Can this OS say which apps use the mic? Asked by the process loop on
    /// every poll: while it can, "a meeting app is open" never asks.
    pub fn list_supported(&self) -> bool {
        self.probed.call_once(|| {
            if matches!(audio::mic_users::mic_users(), MicUsers::Supported(_)) {
                self.supported.store(true, Ordering::SeqCst);
            }
        });
        self.supported.load(Ordering::SeqCst)
    }

    /// May the "mic and speakers both in use" prompt ask now? See
    /// [`detect::call_start::audio_activity_allowed`].
    pub fn audio_activity_allowed(&self) -> bool {
        let reading = lock_or_recover(&self.latest).clone();
        detect::call_start::audio_activity_allowed(&reading, &self.never())
    }

    /// The "Never detect" list now.
    pub fn never(&self) -> Vec<String> {
        let mut never = lock_or_recover(&self.never);
        let now = Instant::now();
        match never.as_ref() {
            Some((apps, at)) if now.saturating_duration_since(*at) < REREAD_AFTER => apps.clone(),
            _ => {
                let apps = (self.read_never)();
                *never = Some((apps.clone(), now));
                apps
            }
        }
    }

    /// The list was just saved: the loop sees it from its next reading.
    pub fn set_never(&self, apps: Vec<String>) {
        *lock_or_recover(&self.never) = Some((apps, Instant::now()));
    }

    /// Not now was pressed on the prompt about `app` (its name).
    pub fn dismissed(&self, app: &str) {
        if let Some(running) = lock_or_recover(&self.running).as_ref() {
            running.dismissed(app);
        }
    }

    fn remember(&self, reading: &MicReading) {
        *lock_or_recover(&self.latest) = reading.clone();
    }
}

/// The `detection.processes` switch as the process loop sees it: on, and
/// only where the OS cannot list the apps on the mic (TUR-143). On when the
/// state is missing, as `super::switch` is.
pub fn process_switch(app: &AppHandle) -> Box<dyn Fn() -> bool + Send> {
    let app = app.clone();
    Box::new(move || {
        app.try_state::<Detection>()
            .is_none_or(|state| processes_on(&state.config(), || state.calls.list_supported()))
    })
}

/// [`process_switch`] without the app: `detection.processes`, and the OS
/// cannot list the apps on the mic (only asked when the switch is on).
fn processes_on(
    config: &crate::config::DetectionConfig,
    list_supported: impl FnOnce() -> bool,
) -> bool {
    config.processes && !list_supported()
}

/// The prompt for `call`: "WhatsApp call detected", "Call detected in
/// Google Chrome".
pub fn signal(call: &CallStarted) -> Signal {
    Signal::Call {
        app: call.name.clone(),
        browser: call.browser,
    }
}

/// The card's line for a call ([`super::notify::headline`]).
pub fn headline(app: &str, browser: bool) -> String {
    if browser {
        format!("Call detected in {app}")
    } else {
        format!("{app} call detected")
    }
}

/// Start the call-start loop. Call it after [`super::start`]. It always
/// runs; with `detection.call_start` and `detection.audio_activity` both off
/// it does not read the OS, and with `call_start` off its prompts are
/// dropped in [`super::notify`].
pub fn start(app: &AppHandle) {
    let Some(state) = app.try_state::<Detection>() else {
        tracing::error!("the detection state is missing; calls starting go unnoticed");
        return;
    };
    let source = SystemMics {
        supported: Arc::clone(&state.calls.supported),
        wanted: wanted(app),
    };
    let (recording_app, never_app, seen_app, notify_app) =
        (app.clone(), app.clone(), app.clone(), app.clone());
    let hooks = MicLoopHooks {
        recording: move || super::notify::recording(&recording_app),
        never: move || {
            never_app
                .try_state::<Detection>()
                .map(|state| state.calls.never())
                .unwrap_or_default()
        },
        seen: move |reading: &MicReading| {
            if let Some(state) = seen_app.try_state::<Detection>() {
                state.calls.remember(reading);
            }
        },
        started: move |call: CallStarted| {
            tracing::info!(app = %call.name, browser = call.browser, "a call started");
            super::notify::notify(&notify_app, &signal(&call));
        },
    };
    match detect::mic_poll::spawn(source, hooks) {
        Ok(running) => {
            tracing::info!("watching which apps use the mic");
            *lock_or_recover(&state.calls.running) = Some(running);
        }
        Err(error) => tracing::warn!(%error, "could not start the call-start watcher"),
    }
}

/// Is any rule that needs the readings switched on?
fn wanted(app: &AppHandle) -> Box<dyn Fn() -> bool + Send> {
    let app = app.clone();
    Box::new(move || {
        app.try_state::<Detection>().is_none_or(|state| {
            let config = state.config();
            config.call_start || config.audio_activity
        })
    })
}

/// The OS's list, through [`audio::mic_users::mic_users`]: property reads
/// only, no stream opened, no permission prompt.
struct SystemMics {
    supported: Arc<AtomicBool>,
    wanted: Box<dyn Fn() -> bool + Send>,
}

impl MicSource for SystemMics {
    fn read(&mut self) -> MicReading {
        if !(self.wanted)() {
            // Switched off: nobody on the mic, and the OS is not asked.
            return MicReading::Supported(Vec::new());
        }
        let reading = reading_of(audio::mic_users::mic_users());
        if matches!(reading, MicReading::Supported(_)) {
            self.supported.store(true, Ordering::SeqCst);
        }
        reading
    }
}

/// `audio`'s answer in `detect`'s types.
fn reading_of(users: MicUsers) -> MicReading {
    match users {
        MicUsers::Supported(apps) => MicReading::Supported(apps.iter().map(user_of).collect()),
        MicUsers::NotSupported => MicReading::NotSupported,
    }
}

fn user_of(app: &MicApp) -> MicUser {
    let kind = match app.kind {
        AppKind::CallApp => MicKind::CallApp,
        AppKind::Browser => MicKind::Browser,
        AppKind::Other => MicKind::Other,
        AppKind::IgnoredSystem => MicKind::Ignored,
    };
    MicUser::new(app.id.clone(), app.name.clone(), kind)
}

/// **Never for <App>** (TUR-147's button): add `name` to
/// `detection.never_detect`. Writes under the meetings root, so through the
/// [`FolderGate`].
pub async fn never_for(app: &AppHandle, name: &str) -> Result<(), UiError> {
    let handle = app.clone();
    let name = name.to_string();
    let saved = on_blocking_pool(move || {
        handle
            .state::<FolderGate>()
            .writing(|| Ok(crate::config::add_never_detect(&name)?))
    })
    .await??;
    if let Some(state) = app.try_state::<Detection>() {
        state.calls.set_never(saved);
    }
    tracing::info!("an app was added to the Never detect list");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mic_app(id: &str, name: &str, kind: AppKind) -> MicApp {
        MicApp {
            pid: 1,
            id: id.to_string(),
            name: name.to_string(),
            kind,
            playing: false,
        }
    }

    #[test]
    fn audio_kinds_become_detect_kinds() {
        let reading = reading_of(MicUsers::Supported(vec![
            mic_app("us.zoom.xos", "Zoom", AppKind::CallApp),
            mic_app("com.google.Chrome", "Google Chrome", AppKind::Browser),
            mic_app("com.example.rec", "Recorder", AppKind::Other),
            mic_app("com.raycast.macos", "Raycast", AppKind::IgnoredSystem),
        ]));
        assert_eq!(
            reading,
            MicReading::Supported(vec![
                MicUser::new("us.zoom.xos", "Zoom", MicKind::CallApp),
                MicUser::new("com.google.Chrome", "Google Chrome", MicKind::Browser),
                MicUser::new("com.example.rec", "Recorder", MicKind::Other),
                MicUser::new("com.raycast.macos", "Raycast", MicKind::Ignored),
            ])
        );
        assert_eq!(reading_of(MicUsers::NotSupported), MicReading::NotSupported);
    }

    #[test]
    fn the_prompt_names_the_app_or_the_browser() {
        let whatsapp = CallStarted {
            id: "net.whatsapp.WhatsApp".to_string(),
            name: "WhatsApp".to_string(),
            browser: false,
        };
        assert_eq!(
            signal(&whatsapp),
            Signal::Call {
                app: "WhatsApp".to_string(),
                browser: false
            }
        );
        assert_eq!(headline("WhatsApp", false), "WhatsApp call detected");
        assert_eq!(
            headline("Google Chrome", true),
            "Call detected in Google Chrome"
        );
    }

    #[test]
    fn audio_activity_follows_the_latest_reading() {
        let calls = Calls::with_never_reader(|| vec!["Recorder".to_string()]);
        // Before any reading: the old rule, as where the list cannot be read.
        assert!(calls.audio_activity_allowed());
        calls.remember(&MicReading::Supported(vec![MicUser::new(
            "ai.krisp.krispMac",
            "Krisp",
            MicKind::Ignored,
        )]));
        assert!(!calls.audio_activity_allowed(), "Krisp is not a call");
        calls.remember(&MicReading::Supported(vec![MicUser::new(
            "com.example.obs",
            "OBS",
            MicKind::Other,
        )]));
        assert!(calls.audio_activity_allowed(), "an app we do not know");
        calls.remember(&MicReading::Supported(vec![MicUser::new(
            "com.example.rec",
            "Recorder",
            MicKind::Other,
        )]));
        assert!(!calls.audio_activity_allowed(), "on the Never detect list");
        calls.remember(&MicReading::NotSupported);
        assert!(calls.audio_activity_allowed());
    }

    #[test]
    fn a_meeting_app_being_open_asks_only_where_the_mic_list_cannot_be_read() {
        use crate::config::DetectionConfig;
        let on = DetectionConfig::default();
        // Teams open all day where the apps on the mic are known: no prompt.
        assert!(!processes_on(&on, || true));
        // macOS older than 14: TUR-27's prompt, as before.
        assert!(processes_on(&on, || false));
        let off = DetectionConfig {
            processes: false,
            ..on
        };
        assert!(!processes_on(&off, || false));
        assert!(
            !processes_on(&off, || panic!("not asked while off")),
            "the OS is not asked while the switch is off"
        );
    }

    #[test]
    fn a_saved_never_list_applies_at_once() {
        let calls = Calls::with_never_reader(Vec::new);
        assert!(calls.never().is_empty());
        calls.set_never(vec!["WhatsApp".to_string()]);
        assert_eq!(calls.never(), ["WhatsApp"]);
    }

    #[test]
    fn a_switched_off_read_does_not_ask_the_os_or_claim_support() {
        let supported = Arc::new(AtomicBool::new(false));
        let mut source = SystemMics {
            supported: Arc::clone(&supported),
            wanted: Box::new(|| false),
        };
        assert_eq!(source.read(), MicReading::Supported(Vec::new()));
        assert!(!supported.load(Ordering::SeqCst));
    }
}
