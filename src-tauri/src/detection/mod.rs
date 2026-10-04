//! Spotting a meeting that has no calendar invite (TUR-27, `docs/problem.md`
//! item 42), and asking before recording it (L15).
//!
//! The rules for *when* to ask live in `crates/detect`; this module runs that
//! crate's loop against the real process list and the real recorder, and
//! sends what it finds through [`notify::notify`], the one prompt path.
//!
//! Audio activity (TUR-31) is a second loop beside it: it reads whether the
//! default mic and speakers are in use, and when both have been for a while
//! it hands that to the process loop, which applies the same don't-nag rules
//! before anything is asked.
//!
//! Calendar reminders (TUR-30, [`reminder`]) are a third loop: a minute
//! before each meeting with enough attendees it asks through the same path,
//! counts as a call signal for Slack and Discord, and [`merge`] makes a
//! reminder and an app prompt for the same call one prompt.
//!
//! TUR-78 made all three follow Settings → Notifications while they run: each
//! loop always starts, and asks the `detection` section ([`live`]) on its own
//! tick. A switch that is off means its source is never read (no process
//! list, no device state, no calendar), and [`notify`] drops a prompt whose
//! switch is off, so turning one off stops it at once. The lead time is the
//! reminder's [`reminder::ReminderSettings`]; [`actions`] are the prompt's
//! Join and Record buttons, and [`settings`] the card's commands.

use std::sync::Mutex;

use detect::{AUDIO_POLL_INTERVAL, ActivityLoop, ActivitySource, RunningProcess};
use detect::{DetectionLoop, POLL_INTERVAL, ProcessSource, Signal, SysinfoProcesses};
use tauri::{AppHandle, Manager as _};

use crate::lock::lock_or_recover;

pub mod actions;
pub mod live;
pub mod merge;
pub mod notify;
pub mod popup;
pub mod reminder;
pub mod settings;

/// Managed state: the running detection loop, kept so it lives as long as the
/// app and so later signals (calendar, audio activity) can reach it.
#[derive(Default)]
pub struct Detection {
    running: Mutex<Option<DetectionLoop>>,
    /// The audio-activity loop (TUR-31), when `detection.audio_activity` is on.
    activity: Mutex<Option<ActivityLoop>>,
    /// The one-minute meeting reminders (TUR-30), when `detection.calendar`
    /// is on.
    reminders: Mutex<Option<reminder::ReminderLoop>>,
    /// The last prompts asked, so one call is asked about once (TUR-30).
    merge: Mutex<merge::Merger>,
    /// The `detection` section as the loops see it (TUR-78).
    live: live::Live,
    /// The events reminded about lately, for the prompt's Join and Record.
    reminded: Mutex<actions::Reminded>,
}

impl Detection {
    /// The `detection` section now (see [`live`]).
    pub fn config(&self) -> crate::config::DetectionConfig {
        self.live.get()
    }
    /// A calendar event was just seen: Slack or Discord being open now counts
    /// as a call. A no-op when detection is off. (Audio activity goes through
    /// [`Self::audio_activity`], which counts as a call signal too.)
    pub fn call_signal(&self) {
        if let Some(running) = lock_or_recover(&self.running).as_ref() {
            running.call_signal();
        }
    }

    /// Run `decide` on the prompt history (see [`merge`]).
    fn merge(&self, decide: impl FnOnce(&mut merge::Merger) -> merge::Delivery) -> merge::Delivery {
        decide(&mut lock_or_recover(&self.merge))
    }

    /// The mic and speakers have both been in use for a while: the process
    /// loop decides whether that asks, and as what.
    fn audio_activity(&self) {
        if let Some(running) = lock_or_recover(&self.running).as_ref() {
            running.audio_activity();
        }
    }
}

/// Start watching for meeting apps. With `detection.processes` off
/// (`processes` is its value at launch) the loop still runs, so switching it
/// on applies at once, but the process list is not read until it is.
pub fn start(app: &AppHandle, processes: bool) {
    if !processes {
        tracing::info!("detection.processes is off; not watching for meeting apps until it is on");
    }
    let recording_app = app.clone();
    let notify_app = app.clone();
    let running = spawn_loop(
        Switched::new(
            SysinfoProcesses::new(),
            switch(app, |config| config.processes),
        ),
        move || notify::recording(&recording_app),
        move |signal| notify::notify(&notify_app, &signal),
    );
    if let Some(running) = running {
        match app.try_state::<Detection>() {
            Some(state) => *lock_or_recover(&state.running) = Some(running),
            None => tracing::error!("the detection state is missing; meeting apps go unnoticed"),
        }
    }
}

/// Start watching the mic and speakers (TUR-31), where this platform can
/// read them. Call it after [`start`]: what it sees goes through the process
/// loop. With `detection.audio_activity` off (`audio_activity` is its value
/// at launch) the devices are not read until it is on.
pub fn start_audio_activity(app: &AppHandle, audio_activity: bool) {
    let Some(state) = app.try_state::<Detection>() else {
        tracing::error!("the detection state is missing; audio activity goes unnoticed");
        return;
    };
    if !audio_activity {
        tracing::info!(
            "detection.audio_activity is off; not watching the mic and speakers until it is on"
        );
    }
    let Some(source) = device_activity() else {
        return;
    };
    let source = Switched::new(source, switch(app, |config| config.audio_activity));
    {
        let mut running = lock_or_recover(&state.running);
        if running.is_none() {
            let recording_app = app.clone();
            let notify_app = app.clone();
            *running = spawn_quiet_loop(
                move || notify::recording(&recording_app),
                move |signal| notify::notify(&notify_app, &signal),
            );
        }
    }
    let recording_app = app.clone();
    let fired_app = app.clone();
    let activity = spawn_activity(
        source,
        move || notify::recording(&recording_app),
        move || {
            if let Some(state) = fired_app.try_state::<Detection>() {
                state.audio_activity();
            }
        },
    );
    *lock_or_recover(&state.activity) = activity;
}

/// Remind before each meeting (TUR-30), as early as
/// `detection.remind_before_minutes` says (TUR-78). Call it after [`start`]:
/// a reminder counts as a call signal for Slack and Discord. The lead time,
/// `detection.min_attendees` and the `detection.calendar` switch are read on
/// every tick (`calendar` is the switch at launch); `calendar.refresh_minutes`
/// once, here.
pub fn start_reminders(app: &AppHandle, calendar: bool) {
    let Some(state) = app.try_state::<Detection>() else {
        tracing::error!("the detection state is missing; meetings go unreminded");
        return;
    };
    if !calendar {
        tracing::info!("detection.calendar is off; no meeting reminders until it is on");
    }
    let fire_app = app.clone();
    let settings_app = app.clone();
    let initial = reminder_settings(&state.config()).unwrap_or_default();
    let reminders = spawn_reminders(
        reminder::AppCalendar(app.clone()),
        reminder::Reminders::new(
            initial,
            chrono::Duration::minutes(i64::from(crate::config::calendar().refresh_minutes)),
        ),
        move || {
            settings_app
                .try_state::<Detection>()
                .and_then(|state| reminder_settings(&state.config()))
        },
        move |event| {
            notify::remind(&fire_app, &event);
            // After the reminder, so a Slack prompt this lets through is
            // merged into it rather than asked first.
            if let Some(state) = fire_app.try_state::<Detection>() {
                state.call_signal();
            }
        },
    );
    *lock_or_recover(&state.reminders) = reminders;
}

/// What the reminder loop works to: `None` with `detection.calendar` off.
pub fn reminder_settings(
    config: &crate::config::DetectionConfig,
) -> Option<reminder::ReminderSettings> {
    config.calendar.then(|| {
        reminder::ReminderSettings::new(config.remind_before_minutes, config.min_attendees as usize)
    })
}

/// [`start_reminders`] without the app, so the loop is testable.
fn spawn_reminders<C: reminder::Upcoming>(
    source: C,
    reminders: reminder::Reminders,
    settings: impl FnMut() -> Option<reminder::ReminderSettings> + Send + 'static,
    fire: impl FnMut(::calendar::Event) + Send + 'static,
) -> Option<reminder::ReminderLoop> {
    match reminder::spawn(
        reminder::SystemClock,
        source,
        reminders,
        reminder::TICK,
        settings,
        fire,
    ) {
        Ok(running) => {
            tracing::info!("reminding before each meeting");
            Some(running)
        }
        Err(error) => {
            tracing::warn!(%error, "could not start the meeting reminders");
            None
        }
    }
}

/// The real mic-and-speakers reader, or `None` when this platform cannot
/// read them (logged once, and no loop starts).
fn device_activity() -> Option<SystemDevices> {
    match audio::activity::device_activity() {
        Err(audio::Error::Unsupported) => {
            tracing::info!("audio-activity detection is not supported on this platform");
            None
        }
        // A failed first read is not fatal: the loop counts it as "not in
        // use" and keeps reading.
        Ok(_) | Err(_) => Some(SystemDevices),
    }
}

/// Reads the default devices through [`audio::activity::device_activity`]:
/// property reads only, no capture and no permission prompt. `detect` does
/// not depend on `audio`, so the reading is copied into its own type here.
struct SystemDevices;

impl ActivitySource for SystemDevices {
    fn read(&mut self) -> Result<detect::AudioReading, detect::Error> {
        audio::activity::device_activity()
            .map(|devices| detect::AudioReading::new(devices.input_running, devices.output_running))
            .map_err(|error| detect::Error::AudioDevices(error.to_string()))
    }
}

/// Whether one `detection` switch is on now, for a [`Switched`] source. On
/// when the state is missing: the loop was started, so it was meant to run.
fn switch(
    app: &AppHandle,
    pick: fn(&crate::config::DetectionConfig) -> bool,
) -> Box<dyn Fn() -> bool + Send> {
    let app = app.clone();
    Box::new(move || {
        app.try_state::<Detection>()
            .is_none_or(|state| pick(&state.config()))
    })
}

/// A source behind a `detection` switch, asked on every read: off, it is
/// not read at all and reports nothing (no apps; mic and speakers idle).
struct Switched<S> {
    inner: S,
    on: Box<dyn Fn() -> bool + Send>,
}

impl<S> Switched<S> {
    fn new(inner: S, on: Box<dyn Fn() -> bool + Send>) -> Self {
        Self { inner, on }
    }
}

impl<S: ProcessSource> ProcessSource for Switched<S> {
    fn running(&mut self) -> Result<Vec<RunningProcess>, detect::Error> {
        if (self.on)() {
            self.inner.running()
        } else {
            Ok(Vec::new())
        }
    }
}

impl<S: ActivitySource> ActivitySource for Switched<S> {
    fn read(&mut self) -> Result<detect::AudioReading, detect::Error> {
        if (self.on)() {
            self.inner.read()
        } else {
            Ok(detect::AudioReading::new(false, false))
        }
    }
}

/// A process list that is always empty: with `detection.processes` off, the
/// loop still runs so audio activity gets the same once-per-call rules, but
/// it never names an app.
struct NoProcesses;

impl ProcessSource for NoProcesses {
    fn running(&mut self) -> Result<Vec<RunningProcess>, detect::Error> {
        Ok(Vec::new())
    }
}

/// The process loop with [`NoProcesses`], for audio activity alone.
fn spawn_quiet_loop(
    recording: impl Fn() -> bool + Send + 'static,
    emit: impl FnMut(Signal) + Send + 'static,
) -> Option<DetectionLoop> {
    detect::spawn(NoProcesses, POLL_INTERVAL, recording, emit)
        .inspect_err(|error| tracing::warn!(%error, "could not start the detection loop"))
        .ok()
}

/// [`start_audio_activity`] without the app, so the loop is testable.
fn spawn_activity<S: ActivitySource>(
    source: S,
    recording: impl Fn() -> bool + Send + 'static,
    fired: impl FnMut() + Send + 'static,
) -> Option<ActivityLoop> {
    match detect::activity::spawn(source, AUDIO_POLL_INTERVAL, recording, fired) {
        Ok(running) => {
            tracing::info!(interval = ?AUDIO_POLL_INTERVAL, "watching the mic and speakers");
            Some(running)
        }
        Err(error) => {
            tracing::warn!(%error, "could not start the audio-activity watcher");
            None
        }
    }
}

/// [`start`] without the app, so the loop is testable.
fn spawn_loop<S: ProcessSource>(
    source: S,
    recording: impl Fn() -> bool + Send + 'static,
    emit: impl FnMut(Signal) + Send + 'static,
) -> Option<DetectionLoop> {
    match detect::spawn(source, POLL_INTERVAL, recording, emit) {
        Ok(running) => {
            tracing::info!(interval = ?POLL_INTERVAL, "watching for meeting apps");
            Some(running)
        }
        Err(error) => {
            tracing::warn!(%error, "could not start the meeting-app watcher");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use detect::AudioReading;

    use super::*;

    struct Counting {
        polls: Arc<AtomicUsize>,
        polled: mpsc::Sender<()>,
    }

    impl ProcessSource for Counting {
        fn running(&mut self) -> Result<Vec<RunningProcess>, detect::Error> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            let _ = self.polled.send(());
            Ok(vec![RunningProcess::new(
                42,
                detect::processes::name_of("Zoom"),
            )])
        }
    }

    /// A switch the test flips, counting how often it was asked.
    fn flip(
        on: bool,
    ) -> (
        Arc<std::sync::atomic::AtomicBool>,
        Arc<AtomicUsize>,
        Box<dyn Fn() -> bool + Send>,
    ) {
        let state = Arc::new(std::sync::atomic::AtomicBool::new(on));
        let asked = Arc::new(AtomicUsize::new(0));
        let (read, count) = (Arc::clone(&state), Arc::clone(&asked));
        let switch = Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            read.load(Ordering::SeqCst)
        });
        (state, asked, switch)
    }

    #[test]
    fn processes_off_never_reads_the_process_list_and_on_applies_live() {
        let polls = Arc::new(AtomicUsize::new(0));
        let counting = || Counting {
            polls: Arc::clone(&polls),
            polled: mpsc::channel().0,
        };
        let (on, _, switch) = flip(false);
        let mut source = Switched::new(counting(), switch);
        assert_eq!(source.running().unwrap(), Vec::new());
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        // On again: read on the next poll, no restart.
        on.store(true, Ordering::SeqCst);
        assert_eq!(source.running().unwrap().len(), 1);
        assert_eq!(polls.load(Ordering::SeqCst), 1);

        // And the loop itself runs while the switch is off, asking it.
        let (_, asked, switch) = flip(false);
        let (signal_tx, signals) = mpsc::channel();
        let running = spawn_loop(
            Switched::new(counting(), switch),
            || false,
            move |signal| {
                let _ = signal_tx.send(signal);
            },
        )
        .expect("the loop runs while the switch is off");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while asked.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        running.stop();
        assert!(asked.load(Ordering::SeqCst) >= 1, "the switch is asked");
        assert_eq!(polls.load(Ordering::SeqCst), 1, "and the list is not read");
        assert!(signals.try_recv().is_err());
    }

    #[test]
    fn processes_on_starts_the_loop_and_it_prompts() {
        let polls = Arc::new(AtomicUsize::new(0));
        let (polled, polled_rx) = mpsc::channel();
        let source = Counting {
            polls: Arc::clone(&polls),
            polled,
        };
        let (signal_tx, signals) = mpsc::channel();
        let running = spawn_loop(
            source,
            || false,
            move |signal| {
                let _ = signal_tx.send(signal);
            },
        )
        .expect("starts");
        polled_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("polled once");
        let signal = signals
            .recv_timeout(Duration::from_secs(10))
            .expect("Zoom prompts");
        assert_eq!(
            signal,
            Signal::Process {
                process: detect::processes::name_of("Zoom")
            }
        );
        running.stop();
    }

    struct Devices {
        reads: Arc<AtomicUsize>,
        read: mpsc::Sender<()>,
    }

    impl ActivitySource for Devices {
        fn read(&mut self) -> Result<AudioReading, detect::Error> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let _ = self.read.send(());
            Ok(AudioReading::new(true, true))
        }
    }

    #[test]
    fn audio_activity_off_never_reads_the_devices_and_never_asks() {
        let reads = Arc::new(AtomicUsize::new(0));
        let (read, _rx) = mpsc::channel();
        let (on, asked, switch) = flip(false);
        let mut source = Switched::new(
            Devices {
                reads: Arc::clone(&reads),
                read,
            },
            switch,
        );
        assert_eq!(source.read().unwrap(), AudioReading::new(false, false));
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        // On again: read on the next poll, no restart.
        on.store(true, Ordering::SeqCst);
        assert_eq!(source.read().unwrap(), AudioReading::new(true, true));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn audio_activity_on_starts_the_loop_and_reads_the_devices() {
        let reads = Arc::new(AtomicUsize::new(0));
        let (read, read_rx) = mpsc::channel();
        let source = Devices {
            reads: Arc::clone(&reads),
            read,
        };
        let running = spawn_activity(source, || false, || {}).expect("starts");
        // The first reading is taken at once; the hold itself is covered by
        // `detect::activity`'s tests with a fake clock.
        read_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("read the devices");
        running.stop();
        assert!(reads.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn the_quiet_loop_never_names_an_app_but_passes_audio_activity_on() {
        let (signal_tx, signals) = mpsc::channel();
        let running = spawn_quiet_loop(
            || false,
            move |signal| {
                let _ = signal_tx.send(signal);
            },
        )
        .expect("starts");
        running.audio_activity();
        assert_eq!(
            signals
                .recv_timeout(Duration::from_secs(10))
                .expect("audio activity asks"),
            Signal::AudioActivity
        );
        running.stop();
        assert!(signals.try_recv().is_err());
    }

    struct CountingCalendar(Arc<AtomicUsize>);

    impl reminder::Upcoming for CountingCalendar {
        fn events_between(
            &self,
            _from: chrono::DateTime<chrono::Utc>,
            _to: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<::calendar::Event>, ::calendar::Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    fn reminders() -> reminder::Reminders {
        reminder::Reminders::new(
            reminder::ReminderSettings::default(),
            chrono::Duration::minutes(15),
        )
    }

    #[test]
    fn calendar_off_means_no_reminder_settings_and_on_takes_the_lead_time() {
        let config = crate::config::DetectionConfig {
            remind_before_minutes: 5,
            min_attendees: 3,
            ..crate::config::DetectionConfig::default()
        };
        assert_eq!(
            reminder_settings(&config),
            Some(reminder::ReminderSettings::new(5, 3))
        );
        assert_eq!(
            reminder_settings(&crate::config::DetectionConfig {
                calendar: false,
                ..config
            }),
            None
        );
    }

    #[test]
    fn calendar_on_starts_the_reminders_and_reads_the_calendar() {
        let reads = Arc::new(AtomicUsize::new(0));
        let running = spawn_reminders(
            CountingCalendar(Arc::clone(&reads)),
            reminders(),
            || Some(reminder::ReminderSettings::default()),
            |_| {},
        )
        .expect("starts");
        // The first tick reads at once; the rules are `reminder`'s tests.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while reads.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        running.stop();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
