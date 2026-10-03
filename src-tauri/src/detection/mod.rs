//! Spotting a meeting that has no calendar invite (TUR-27, `docs/problem.md`
//! item 42), and asking before recording it (L15).
//!
//! The rules for *when* to ask live in `crates/detect`; this module runs that
//! crate's loop against the real process list and the real recorder, and
//! sends what it finds through [`notify::notify`], the one prompt path.

use std::sync::Mutex;

use detect::{DetectionLoop, POLL_INTERVAL, ProcessSource, Signal, SysinfoProcesses};
use tauri::{AppHandle, Manager as _};

use crate::lock::lock_or_recover;

pub mod notify;

/// `detection.processes` (SPEC §3.5), until TUR-25's detection config section
/// is there to read it from: on.
pub const PROCESSES_DEFAULT: bool = true;

/// Managed state: the running detection loop, kept so it lives as long as the
/// app and so later signals (calendar, audio activity) can reach it.
#[derive(Default)]
pub struct Detection {
    running: Mutex<Option<DetectionLoop>>,
}

impl Detection {
    /// A calendar event or audio activity was just seen: Slack or Discord
    /// being open now counts as a call. A no-op when detection is off.
    #[allow(dead_code)] // TUR-26 (calendar) and audio activity call this.
    pub fn call_signal(&self) {
        if let Some(running) = lock_or_recover(&self.running).as_ref() {
            running.call_signal();
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

    use detect::RunningProcess;

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
}
