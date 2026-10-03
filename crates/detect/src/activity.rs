//! Audio activity: the mic and the speakers both in use for a while, which is
//! what a call looks like even when no known meeting app is running — Google
//! Meet in a browser tab, say (TUR-31, `docs/problem.md` item 42).
//!
//! The rules, all in [`ActivityHold`] (pure: readings and a time in, a yes/no
//! out, so each is a unit test with fake readings):
//!
//! 1. **Both, held.** It fires only after the default input *and* the default
//!    output have both been running for [`AUDIO_HOLD`]. Music alone (output
//!    only) or a mic check alone (input only) never fires, and any reading
//!    with either one off restarts the window.
//! 2. **Not while recording.** meet-ai's own mic stream makes the input
//!    "running", so readings taken while recording say nothing. And like an
//!    app seen while recording (see [`crate::detector`]), a call that was
//!    going on during a recording counts as handled.
//! 3. **Once per call.** After firing (or being handled by a recording) it
//!    re-arms only once mic and speakers have not both been running for
//!    [`AUDIO_REARM`] in a row — a call hanging up. A muted mic or a brief
//!    glitch mid-call does not ask again.
//!
//! [`spawn`] runs the readings on a thread every [`AUDIO_POLL_INTERVAL`]. What
//! it fires is not a prompt yet: the app hands it to the process loop
//! ([`crate::DetectionLoop::audio_activity`]), which decides between naming a
//! meeting app, saying "audio activity", or staying quiet.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::Error;

/// How often the devices are read. Two seconds: well inside [`AUDIO_HOLD`],
/// and each read is four Core Audio property reads.
pub const AUDIO_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long mic and speakers must both be running before it counts as a
/// call. Long enough to skip a voice memo or a notification sound over a
/// dictation; short enough to ask before anything worth keeping is said.
pub const AUDIO_HOLD: Duration = Duration::from_secs(20);

/// How long mic and speakers must stay apart after a call before the next
/// call can ask.
pub const AUDIO_REARM: Duration = Duration::from_secs(60);

/// One reading of the default devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioReading {
    /// Some process is using the default input device (a mic).
    pub input_running: bool,
    /// Some process is using the default output device.
    pub output_running: bool,
}

impl AudioReading {
    pub fn new(input_running: bool, output_running: bool) -> Self {
        Self {
            input_running,
            output_running,
        }
    }

    fn both(self) -> bool {
        self.input_running && self.output_running
    }
}

/// Where readings come from. The app's is Core Audio
/// (`audio::macos::activity`); tests pass a fake.
pub trait ActivitySource: Send + 'static {
    fn read(&mut self) -> Result<AudioReading, Error>;
}

/// What audio-activity detection remembers between readings.
#[derive(Debug, Default)]
pub struct ActivityHold {
    /// When the current both-running stretch began, while armed.
    since: Option<Instant>,
    /// This call already fired, or ran during a recording.
    handled: bool,
    /// While handled: when mic and speakers stopped both running.
    quiet_since: Option<Instant>,
}

impl ActivityHold {
    pub fn new() -> Self {
        Self::default()
    }

    /// One reading at `now`, with whether meet-ai is recording. `true` means
    /// a call just started: at most once per call.
    pub fn observe(&mut self, reading: AudioReading, recording: bool, now: Instant) -> bool {
        if recording {
            self.since = None;
            self.handled = true;
            self.quiet_since = None;
            return false;
        }
        if self.handled {
            if reading.both() {
                self.quiet_since = None;
            } else {
                let quiet = *self.quiet_since.get_or_insert(now);
                if now.saturating_duration_since(quiet) >= AUDIO_REARM {
                    self.handled = false;
                    self.quiet_since = None;
                }
            }
            return false;
        }
        if !reading.both() {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= AUDIO_HOLD {
            self.since = None;
            self.handled = true;
            return true;
        }
        false
    }
}

/// A running audio-activity loop. Dropping it stops the loop and waits for it.
pub struct ActivityLoop {
    tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl ActivityLoop {
    /// Stop the loop and wait for its thread.
    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        let _ = self.tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ActivityLoop {
    fn drop(&mut self) {
        self.shut_down();
    }
}

/// Read `source` every `interval` on its own thread, and call `fired` each
/// time [`ActivityHold`] says a call started. `recording` is asked on every
/// reading. A failed read counts as "not both running" (it restarts the
/// window); only the first failure in a row is logged as a warning.
pub fn spawn<S, R, F>(
    source: S,
    interval: Duration,
    recording: R,
    fired: F,
) -> std::io::Result<ActivityLoop>
where
    S: ActivitySource,
    R: Fn() -> bool + Send + 'static,
    F: FnMut() + Send + 'static,
{
    spawn_with_clock(source, interval, recording, fired, Instant::now)
}

/// [`spawn`] with the clock passed in, so a test can run 20 s of readings in
/// a few milliseconds.
fn spawn_with_clock<S, R, F, C>(
    mut source: S,
    interval: Duration,
    recording: R,
    mut fired: F,
    mut clock: C,
) -> std::io::Result<ActivityLoop>
where
    S: ActivitySource,
    R: Fn() -> bool + Send + 'static,
    F: FnMut() + Send + 'static,
    C: FnMut() -> Instant + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("meet-ai-audio-activity".to_string())
        .spawn(move || {
            let mut hold = ActivityHold::new();
            let mut failing = false;
            loop {
                let reading = match source.read() {
                    Ok(reading) => {
                        failing = false;
                        reading
                    }
                    Err(error) => {
                        if failing {
                            tracing::debug!(%error, "could not read audio device activity");
                        } else {
                            tracing::warn!(%error, "could not read audio device activity");
                        }
                        failing = true;
                        AudioReading::default()
                    }
                };
                if hold.observe(reading, recording(), clock()) {
                    fired();
                }
                match rx.recv_timeout(interval) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
    Ok(ActivityLoop {
        tx,
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    const STEP: Duration = AUDIO_POLL_INTERVAL;

    /// Feed `readings` one per [`STEP`] from `start`; the indexes that fired.
    fn run(
        hold: &mut ActivityHold,
        start: Instant,
        readings: &[(AudioReading, bool)],
    ) -> Vec<usize> {
        readings
            .iter()
            .enumerate()
            .filter(|(i, (reading, recording))| {
                hold.observe(*reading, *recording, start + STEP * (*i as u32))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn steps(reading: AudioReading, recording: bool, n: usize) -> Vec<(AudioReading, bool)> {
        vec![(reading, recording); n]
    }

    fn call() -> AudioReading {
        AudioReading::new(true, true)
    }
    fn music() -> AudioReading {
        AudioReading::new(false, true)
    }
    fn mic_check() -> AudioReading {
        AudioReading::new(true, false)
    }
    fn silence() -> AudioReading {
        AudioReading::new(false, false)
    }

    /// Readings at [`STEP`] that span exactly [`AUDIO_HOLD`]: the last fires.
    fn hold_steps() -> usize {
        (AUDIO_HOLD.as_secs() / STEP.as_secs()) as usize + 1
    }

    #[test]
    fn mic_and_speakers_held_fire_once() {
        let mut hold = ActivityHold::new();
        let fired = run(&mut hold, Instant::now(), &steps(call(), false, 300));
        assert_eq!(fired, [hold_steps() - 1]);
    }

    #[test]
    fn just_short_of_the_hold_does_not_fire() {
        let mut hold = ActivityHold::new();
        let start = Instant::now();
        assert!(!hold.observe(call(), false, start));
        assert!(!hold.observe(call(), false, start + AUDIO_HOLD - Duration::from_millis(1)));
        assert!(hold.observe(call(), false, start + AUDIO_HOLD));
    }

    #[test]
    fn music_alone_never_fires() {
        let mut hold = ActivityHold::new();
        assert!(run(&mut hold, Instant::now(), &steps(music(), false, 1800)).is_empty());
    }

    #[test]
    fn a_mic_check_alone_never_fires() {
        let mut hold = ActivityHold::new();
        assert!(run(&mut hold, Instant::now(), &steps(mic_check(), false, 1800)).is_empty());
    }

    #[test]
    fn silence_never_fires() {
        let mut hold = ActivityHold::new();
        assert!(run(&mut hold, Instant::now(), &steps(silence(), false, 100)).is_empty());
    }

    #[test]
    fn any_false_reading_restarts_the_window() {
        let mut hold = ActivityHold::new();
        // 18 s of call, one music-only reading, then the window starts over.
        let mut readings = steps(call(), false, hold_steps() - 1);
        readings.push((music(), false));
        readings.extend(steps(call(), false, hold_steps() - 1));
        readings.push((mic_check(), false));
        readings.extend(steps(call(), false, hold_steps()));
        let fired = run(&mut hold, Instant::now(), &readings);
        assert_eq!(fired, [readings.len() - 1]);
    }

    #[test]
    fn alternating_mic_and_speakers_never_fires() {
        let mut hold = ActivityHold::new();
        let readings: Vec<_> = (0..600)
            .map(|i| (if i % 2 == 0 { music() } else { mic_check() }, false))
            .collect();
        assert!(run(&mut hold, Instant::now(), &readings).is_empty());
    }

    #[test]
    fn a_glitch_mid_call_does_not_ask_again() {
        let mut hold = ActivityHold::new();
        let mut readings = steps(call(), false, hold_steps());
        // Muted for 30 s, back on: the same call.
        readings.extend(steps(music(), false, 15));
        readings.extend(steps(call(), false, 100));
        assert_eq!(
            run(&mut hold, Instant::now(), &readings),
            [hold_steps() - 1]
        );
    }

    #[test]
    fn the_next_call_asks_after_a_quiet_minute() {
        let mut hold = ActivityHold::new();
        let quiet = (AUDIO_REARM.as_secs() / STEP.as_secs()) as usize + 1;
        let mut readings = steps(call(), false, hold_steps());
        readings.extend(steps(silence(), false, quiet));
        readings.extend(steps(call(), false, hold_steps()));
        let fired = run(&mut hold, Instant::now(), &readings);
        assert_eq!(fired, [hold_steps() - 1, readings.len() - 1]);
    }

    #[test]
    fn no_fire_while_recording() {
        let mut hold = ActivityHold::new();
        assert!(run(&mut hold, Instant::now(), &steps(call(), true, 300)).is_empty());
    }

    #[test]
    fn a_call_seen_while_recording_does_not_ask_when_the_recording_stops() {
        let mut hold = ActivityHold::new();
        let mut readings = steps(call(), true, 50);
        readings.extend(steps(call(), false, 300));
        assert!(run(&mut hold, Instant::now(), &readings).is_empty());
    }

    #[test]
    fn after_a_recording_the_next_call_asks_once_it_is_quiet() {
        let mut hold = ActivityHold::new();
        let quiet = (AUDIO_REARM.as_secs() / STEP.as_secs()) as usize + 1;
        let mut readings = steps(call(), true, 50);
        readings.extend(steps(silence(), false, quiet));
        readings.extend(steps(call(), false, hold_steps()));
        assert_eq!(
            run(&mut hold, Instant::now(), &readings),
            [readings.len() - 1]
        );
    }

    #[test]
    fn a_recording_mid_window_restarts_it() {
        let mut hold = ActivityHold::new();
        let start = Instant::now();
        assert!(!hold.observe(call(), false, start));
        assert!(!hold.observe(call(), true, start + Duration::from_secs(10)));
        // Handled now: even long after, the same call does not ask.
        assert!(!hold.observe(call(), false, start + Duration::from_secs(40)));
    }

    /// Hands out one scripted reading per poll (the last one repeats), and
    /// reports each poll so a test can wait for it.
    struct Scripted {
        readings: Vec<Result<AudioReading, ()>>,
        polled: mpsc::Sender<()>,
    }

    impl ActivitySource for Scripted {
        fn read(&mut self) -> Result<AudioReading, Error> {
            let next = if self.readings.len() > 1 {
                self.readings.remove(0)
            } else {
                self.readings.first().copied().unwrap_or(Ok(silence()))
            };
            let _ = self.polled.send(());
            next.map_err(|()| Error::AudioDevices("scripted failure".to_string()))
        }
    }

    /// A clock that moves one [`STEP`] per call.
    fn stepping_clock() -> impl FnMut() -> Instant + Send + 'static {
        let start = Instant::now();
        let mut n = 0u32;
        move || {
            n += 1;
            start + STEP * n
        }
    }

    fn wait_polls(polled: &mpsc::Receiver<()>, n: usize) {
        for _ in 0..n {
            polled
                .recv_timeout(Duration::from_secs(10))
                .expect("the loop kept reading");
        }
    }

    #[test]
    fn the_loop_fires_once_for_a_held_call_and_survives_read_errors() {
        let (polled_tx, polled) = mpsc::channel();
        let source = Scripted {
            readings: vec![Err(()), Err(()), Ok(call())],
            polled: polled_tx,
        };
        let fired = Arc::new(Mutex::new(0));
        let count = Arc::clone(&fired);
        let running = spawn_with_clock(
            source,
            Duration::from_millis(1),
            || false,
            move || *count.lock().expect("not poisoned") += 1,
            stepping_clock(),
        )
        .expect("spawns");
        wait_polls(&polled, 2 + hold_steps() * 3);
        running.stop();
        assert_eq!(*fired.lock().expect("not poisoned"), 1);
    }

    #[test]
    fn the_loop_reads_the_recording_state_on_every_poll() {
        let (polled_tx, polled) = mpsc::channel();
        let source = Scripted {
            readings: vec![Ok(call())],
            polled: polled_tx,
        };
        let fired = Arc::new(Mutex::new(0));
        let count = Arc::clone(&fired);
        let running = spawn_with_clock(
            source,
            Duration::from_millis(1),
            || true,
            move || *count.lock().expect("not poisoned") += 1,
            stepping_clock(),
        )
        .expect("spawns");
        wait_polls(&polled, hold_steps() * 3);
        running.stop();
        assert_eq!(*fired.lock().expect("not poisoned"), 0);
    }
}
