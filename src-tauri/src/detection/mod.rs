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

use std::sync::Mutex;

use detect::{AUDIO_POLL_INTERVAL, ActivityLoop, ActivitySource, RunningProcess};
use detect::{DetectionLoop, POLL_INTERVAL, ProcessSource, Signal, SysinfoProcesses};
use tauri::{AppHandle, Manager as _};

use crate::lock::lock_or_recover;

pub mod notify;

/// Managed state: the running detection loop, kept so it lives as long as the
/// app and so later signals (calendar, audio activity) can reach it.
#[derive(Default)]
pub struct Detection {
    running: Mutex<Option<DetectionLoop>>,
    /// The audio-activity loop (TUR-31), when `detection.audio_activity` is on.
    activity: Mutex<Option<ActivityLoop>>,
}

impl Detection {
    /// A calendar event was just seen: Slack or Discord being open now counts
    /// as a call. A no-op when detection is off. (Audio activity goes through
    /// [`Self::audio_activity`], which counts as a call signal too.)
    #[allow(dead_code)] // The calendar refresh loop (TUR-30) calls this.
    pub fn call_signal(&self) {
        if let Some(running) = lock_or_recover(&self.running).as_ref() {
            running.call_signal();
        }
    }

    /// The mic and speakers have both been in use for a while: the process
    /// loop decides whether that asks, and as what.
    fn audio_activity(&self) {
        if let Some(running) = lock_or_recover(&self.running).as_ref() {
            running.audio_activity();
        }
    }
}

/// Start watching for meeting apps, unless `processes` (the
/// `detection.processes` switch) is off.
pub fn start(app: &AppHandle, processes: bool) {
    let recording_app = app.clone();
    let notify_app = app.clone();
    let running = spawn_loop(
        processes,
        SysinfoProcesses::new(),
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

/// Start watching the mic and speakers (TUR-31), unless `audio_activity` (the
/// `detection.audio_activity` switch) is off. Call it after [`start`]: what it
/// sees goes through the process loop, and with `detection.processes` off it
/// starts one that sees no apps, so audio activity still asks once per call.
pub fn start_audio_activity(app: &AppHandle, audio_activity: bool) {
    let Some(state) = app.try_state::<Detection>() else {
        tracing::error!("the detection state is missing; audio activity goes unnoticed");
        return;
    };
    let Some(source) = device_activity(audio_activity) else {
        return;
    };
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
        audio_activity,
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

/// The real mic-and-speakers reader, or `None` when the switch is off or
/// this platform cannot read them (logged once, and no loop starts).
fn device_activity(audio_activity: bool) -> Option<SystemDevices> {
    if !audio_activity {
        tracing::info!("detection.audio_activity is off; not watching the mic and speakers");
        return None;
    }
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

/// [`start_audio_activity`] without the app, so the switch is testable:
/// `None`, and the devices never read, when `audio_activity` is off.
fn spawn_activity<S: ActivitySource>(
    audio_activity: bool,
    source: S,
    recording: impl Fn() -> bool + Send + 'static,
    fired: impl FnMut() + Send + 'static,
) -> Option<ActivityLoop> {
    if !audio_activity {
        return None;
    }
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

/// [`start`] without the app, so the switch is testable: `None`, and the
/// process list never read, when `processes` is off.
fn spawn_loop<S: ProcessSource>(
    processes: bool,
    source: S,
    recording: impl Fn() -> bool + Send + 'static,
    emit: impl FnMut(Signal) + Send + 'static,
) -> Option<DetectionLoop> {
    if !processes {
        tracing::info!("detection.processes is off; not watching for meeting apps");
        return None;
    }
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
            Ok(vec![RunningProcess::new(42, "zoom.us")])
        }
    }

    #[test]
    fn processes_off_means_no_loop_and_no_process_list() {
        let polls = Arc::new(AtomicUsize::new(0));
        let (polled, _rx) = mpsc::channel();
        let source = Counting {
            polls: Arc::clone(&polls),
            polled,
        };
        let running = spawn_loop(false, source, || false, |_| panic!("no prompt"));
        assert!(running.is_none());
        assert_eq!(polls.load(Ordering::SeqCst), 0);
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
            true,
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
                process: "zoom.us".to_string()
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
    fn audio_activity_off_means_no_loop_and_no_reads() {
        let reads = Arc::new(AtomicUsize::new(0));
        let (read, _rx) = mpsc::channel();
        let source = Devices {
            reads: Arc::clone(&reads),
            read,
        };
        let running = spawn_activity(false, source, || false, || panic!("no prompt"));
        assert!(running.is_none());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn audio_activity_on_starts_the_loop_and_reads_the_devices() {
        let reads = Arc::new(AtomicUsize::new(0));
        let (read, read_rx) = mpsc::channel();
        let source = Devices {
            reads: Arc::clone(&reads),
            read,
        };
        let running = spawn_activity(true, source, || false, || {}).expect("starts");
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
}
