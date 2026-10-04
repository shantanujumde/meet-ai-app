//! The detection loop: list the running processes every [`POLL_INTERVAL`],
//! run them through a [`Detector`], and hand each resulting [`Signal`] to
//! the caller.
//!
//! The loop never records anything (L15). What it emits is a reason to *ask*;
//! the app turns that into a notification and the user decides.

use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::detector::{Detector, RunningProcess};
use crate::worker::Worker;
use crate::{Error, Signal};

/// How often the process list is read. Five seconds: a call is noticed well
/// before anyone says anything worth recording, and one process-list read
/// every five seconds is nothing next to what the meeting app itself costs.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Where the running processes come from. The real one is
/// [`SysinfoProcesses`]; tests pass a fake.
pub trait ProcessSource: Send + 'static {
    fn running(&mut self) -> Result<Vec<RunningProcess>, Error>;
}

/// The OS process list, through `sysinfo` (SPEC §2.3).
pub struct SysinfoProcesses {
    system: System,
}

impl SysinfoProcesses {
    pub fn new() -> Self {
        Self {
            system: System::new(),
        }
    }
}

impl Default for SysinfoProcesses {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSource for SysinfoProcesses {
    fn running(&mut self) -> Result<Vec<RunningProcess>, Error> {
        // Names and pids only: no CPU, memory, disk or environment, which
        // would cost more and tell detection nothing.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        Ok(self
            .system
            .processes()
            .iter()
            .map(|(pid, process)| {
                RunningProcess::new(pid.as_u32(), process.name().to_string_lossy())
            })
            .collect())
    }
}

enum Message {
    CallSignal(Instant),
    AudioActivity(Instant),
}

/// A running detection loop. Dropping it stops the loop and waits for it.
pub struct DetectionLoop {
    worker: Worker<Message>,
}

impl DetectionLoop {
    /// A calendar event was just seen, so Slack or Discord being open now
    /// counts as a call (see [`crate::detector`]). Polls at once rather than
    /// waiting out the interval. (Audio activity has its own
    /// [`Self::audio_activity`], which counts as a call signal too.)
    pub fn call_signal(&self) {
        self.worker.send(Message::CallSignal(Instant::now()));
    }

    /// The mic and speakers have both been in use for a while (see
    /// [`crate::activity`]). Polls at once, and prompts for a meeting app it
    /// explains or else for [`Signal::AudioActivity`] (see
    /// [`Detector::audio_activity`]).
    pub fn audio_activity(&self) {
        self.worker.send(Message::AudioActivity(Instant::now()));
    }

    /// Stop the loop and wait for its thread.
    pub fn stop(self) {
        self.worker.stop();
    }
}

/// Start polling `source` every `interval` on its own thread.
///
/// `recording` is asked on every poll, so a recording started anywhere (the
/// button, ⌘⇧R, the menu bar) silences detection at once. `emit` gets each
/// signal worth asking about.
pub fn spawn<S, R, E>(
    mut source: S,
    interval: Duration,
    recording: R,
    mut emit: E,
) -> std::io::Result<DetectionLoop>
where
    S: ProcessSource,
    R: Fn() -> bool + Send + 'static,
    E: FnMut(Signal) + Send + 'static,
{
    let mut detector = Detector::new();
    // Audio activity waiting for a process list to be weighed against; kept
    // across a failed read, so it is not lost.
    let mut audio_activity: Option<Instant> = None;
    let worker = Worker::spawn("meet-ai-detection", interval, move |message| {
        match message {
            Some(Message::CallSignal(at)) => detector.call_signal(at),
            Some(Message::AudioActivity(at)) => audio_activity = Some(at),
            None => {}
        }
        match source.running() {
            Ok(running) => {
                let signals = match audio_activity.take() {
                    Some(at) => detector.audio_activity(&running, recording(), at),
                    None => detector.observe(&running, recording(), Instant::now()),
                };
                for signal in signals {
                    emit(signal);
                }
            }
            Err(error) => tracing::warn!(%error, "could not list running processes"),
        }
    })?;
    Ok(DetectionLoop { worker })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, mpsc};

    use super::*;
    use crate::processes::name_of;

    /// Hands out one scripted process list per poll (the last one repeats),
    /// and reports each poll on a channel so a test can wait for it.
    struct Scripted {
        polls: Vec<Vec<RunningProcess>>,
        polled: mpsc::Sender<()>,
    }

    impl ProcessSource for Scripted {
        fn running(&mut self) -> Result<Vec<RunningProcess>, Error> {
            let list = if self.polls.len() > 1 {
                self.polls.remove(0)
            } else {
                self.polls.first().cloned().unwrap_or_default()
            };
            let _ = self.polled.send(());
            Ok(list)
        }
    }

    fn wait_polls(polled: &mpsc::Receiver<()>, n: usize) {
        for _ in 0..n {
            polled
                .recv_timeout(Duration::from_secs(10))
                .expect("the loop kept polling");
        }
    }

    #[test]
    fn the_loop_emits_one_process_signal_per_session() {
        let (polled_tx, polled) = mpsc::channel();
        let zoom = || vec![RunningProcess::new(42, name_of("Zoom"))];
        let source = Scripted {
            polls: vec![vec![], zoom(), zoom(), vec![], zoom()],
            polled: polled_tx,
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let running = spawn(
            source,
            Duration::from_millis(1),
            || false,
            move |signal| {
                sink.lock().expect("not poisoned").push(signal);
            },
        )
        .expect("spawns");
        wait_polls(&polled, 8);
        running.stop();

        let zoom_signal = Signal::Process {
            process: name_of("Zoom").to_string(),
        };
        assert_eq!(
            *seen.lock().expect("not poisoned"),
            [zoom_signal.clone(), zoom_signal]
        );
    }

    #[test]
    fn the_loop_reads_the_recording_state_on_every_poll() {
        let (polled_tx, polled) = mpsc::channel();
        let source = Scripted {
            polls: vec![vec![RunningProcess::new(42, name_of("Zoom"))]],
            polled: polled_tx,
        };
        let recording = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&recording);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let running = spawn(
            source,
            Duration::from_millis(1),
            move || flag.load(Ordering::SeqCst),
            move |signal| sink.lock().expect("not poisoned").push(signal),
        )
        .expect("spawns");
        wait_polls(&polled, 3);
        recording.store(false, Ordering::SeqCst);
        wait_polls(&polled, 3);
        running.stop();
        assert!(seen.lock().expect("not poisoned").is_empty());
    }

    #[test]
    fn a_call_signal_lets_slack_prompt() {
        let (polled_tx, polled) = mpsc::channel();
        let source = Scripted {
            polls: vec![vec![RunningProcess::new(5, name_of("Slack"))]],
            polled: polled_tx,
        };
        let (signal_tx, signals) = mpsc::channel();
        // A long interval: only the call signal can wake the loop for poll 2.
        let running = spawn(
            source,
            Duration::from_secs(3600),
            || false,
            move |signal| {
                let _ = signal_tx.send(signal);
            },
        )
        .expect("spawns");
        wait_polls(&polled, 1);
        assert!(signals.try_recv().is_err());
        running.call_signal();
        wait_polls(&polled, 1);
        let signal = signals
            .recv_timeout(Duration::from_secs(10))
            .expect("Slack prompts after the call signal");
        assert_eq!(
            signal,
            Signal::Process {
                process: name_of("Slack").to_string()
            }
        );
        drop(running);
    }

    #[test]
    fn audio_activity_prompts_once_with_no_meeting_app() {
        let (polled_tx, polled) = mpsc::channel();
        let source = Scripted {
            polls: vec![vec![RunningProcess::new(1, "Google Chrome")]],
            polled: polled_tx,
        };
        let (signal_tx, signals) = mpsc::channel();
        let running = spawn(
            source,
            Duration::from_secs(3600),
            || false,
            move |signal| {
                let _ = signal_tx.send(signal);
            },
        )
        .expect("spawns");
        wait_polls(&polled, 1);
        running.audio_activity();
        wait_polls(&polled, 1);
        assert_eq!(
            signals
                .recv_timeout(Duration::from_secs(10))
                .expect("audio activity prompts"),
            Signal::AudioActivity
        );
        running.audio_activity();
        wait_polls(&polled, 1);
        running.stop();
        assert!(signals.try_recv().is_err(), "asked only once");
    }

    #[test]
    #[ignore = "reads the real process list of this Mac"]
    fn sysinfo_lists_this_process() {
        let list = SysinfoProcesses::new().running().expect("lists");
        assert!(list.iter().any(|p| p.pid == std::process::id()));
    }
}
