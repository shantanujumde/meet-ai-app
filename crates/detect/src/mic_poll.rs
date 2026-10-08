//! The call-start loop (TUR-143): read who uses the mic every
//! [`MIC_POLL_INTERVAL`], hand every reading to the caller, and run it
//! through a [`CallStart`] for the calls worth asking about.
//!
//! The loop never records anything (L15). What it emits is a reason to ask.

use std::time::{Duration, Instant};

use crate::call_start::{CallStart, CallStarted, MIC_POLL_INTERVAL, MicReading};
use crate::worker::Worker;

/// Where readings come from. The app's is `audio::mic_users`; tests pass a
/// fake.
pub trait MicSource: Send + 'static {
    fn read(&mut self) -> MicReading;
}

enum Message {
    /// Not now was pressed on the prompt about this app (its name).
    Dismissed(String, Instant),
}

/// A running call-start loop. Dropping it stops the loop and waits for it.
pub struct MicLoop {
    worker: Worker<Message>,
}

impl MicLoop {
    /// The user pressed Not now on the prompt about `app` (the name the
    /// prompt showed): see [`CallStart::dismissed`].
    pub fn dismissed(&self, app: &str) {
        self.worker
            .send(Message::Dismissed(app.to_string(), Instant::now()));
    }

    /// Stop the loop and wait for its thread.
    pub fn stop(self) {
        self.worker.stop();
    }
}

/// What the loop asks the app on every reading.
pub struct MicLoopHooks<R, N, S, E> {
    /// Is meet-ai recording?
    pub recording: R,
    /// The user's "Never detect" list now.
    pub never: N,
    /// Every reading, before the rule sees it.
    pub seen: S,
    /// A call worth asking about.
    pub started: E,
}

/// Read `source` every [`MIC_POLL_INTERVAL`] on its own thread. A reading
/// the OS could not give ([`MicReading::NotSupported`]) counts as nobody on
/// the mic for the rule, and still reaches `seen`.
pub fn spawn<M, R, N, S, E>(source: M, hooks: MicLoopHooks<R, N, S, E>) -> std::io::Result<MicLoop>
where
    M: MicSource,
    R: Fn() -> bool + Send + 'static,
    N: Fn() -> Vec<String> + Send + 'static,
    S: FnMut(&MicReading) + Send + 'static,
    E: FnMut(CallStarted) + Send + 'static,
{
    spawn_with_clock(source, MIC_POLL_INTERVAL, hooks, Instant::now)
}

/// [`spawn`] with the interval and clock passed in, so a test can run 15 s
/// of readings in a few milliseconds.
fn spawn_with_clock<M, R, N, S, E, C>(
    mut source: M,
    interval: Duration,
    hooks: MicLoopHooks<R, N, S, E>,
    mut clock: C,
) -> std::io::Result<MicLoop>
where
    M: MicSource,
    R: Fn() -> bool + Send + 'static,
    N: Fn() -> Vec<String> + Send + 'static,
    S: FnMut(&MicReading) + Send + 'static,
    E: FnMut(CallStarted) + Send + 'static,
    C: FnMut() -> Instant + Send + 'static,
{
    let MicLoopHooks {
        recording,
        never,
        mut seen,
        mut started,
    } = hooks;
    let mut rule = CallStart::new();
    let worker = Worker::spawn("meet-ai-call-start", interval, move |message| {
        if let Some(Message::Dismissed(app, at)) = message {
            rule.dismissed(&app, at);
            return;
        }
        let reading = source.read();
        seen(&reading);
        let users = match &reading {
            MicReading::Supported(users) => users.as_slice(),
            MicReading::NotSupported => &[],
        };
        if let Some(call) = rule.observe(users, &never(), recording(), clock()) {
            started(call);
        }
    })?;
    Ok(MicLoop { worker })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use super::*;
    use crate::call_start::{CALL_START_HOLD, MicKind, MicUser};

    /// Always the same reading; each read reported on a channel.
    struct Fixed {
        reading: MicReading,
        read: mpsc::Sender<()>,
    }

    impl MicSource for Fixed {
        fn read(&mut self) -> MicReading {
            let _ = self.read.send(());
            self.reading.clone()
        }
    }

    fn whatsapp() -> MicUser {
        MicUser::new("net.whatsapp.WhatsApp", "WhatsApp", MicKind::CallApp)
    }

    /// A clock that moves 2 s on every reading.
    fn stepping() -> impl FnMut() -> Instant + Send + 'static {
        let start = Instant::now();
        let mut ticks = 0u32;
        move || {
            ticks += 1;
            start + MIC_POLL_INTERVAL * ticks
        }
    }

    fn wait_reads(read: &mpsc::Receiver<()>, n: usize) {
        for _ in 0..n {
            read.recv_timeout(Duration::from_secs(10))
                .expect("the loop kept reading");
        }
    }

    #[test]
    fn a_call_held_past_15_s_asks_once_and_every_reading_is_seen() {
        let (read_tx, read) = mpsc::channel();
        let source = Fixed {
            reading: MicReading::Supported(vec![whatsapp()]),
            read: read_tx,
        };
        let seen = Arc::new(Mutex::new(0usize));
        let seen_count = Arc::clone(&seen);
        let (started_tx, started) = mpsc::channel();
        let running = spawn_with_clock(
            source,
            Duration::from_millis(1),
            MicLoopHooks {
                recording: || false,
                never: Vec::new,
                seen: move |_: &MicReading| *seen_count.lock().expect("not poisoned") += 1,
                started: move |call| {
                    let _ = started_tx.send(call);
                },
            },
            stepping(),
        )
        .expect("spawns");
        let polls = usize::try_from(CALL_START_HOLD.as_secs() / 2 + 5).expect("small");
        wait_reads(&read, polls);
        running.stop();
        let call = started.recv_timeout(Duration::from_secs(10)).expect("asks");
        assert_eq!(call.name, "WhatsApp");
        assert!(!call.browser);
        assert!(started.try_recv().is_err(), "asked once");
        assert!(*seen.lock().expect("not poisoned") >= polls);
    }

    #[test]
    fn the_never_list_and_recording_are_asked_on_every_reading() {
        for (recording, never) in [(true, Vec::new()), (false, vec!["WhatsApp".to_string()])] {
            let (read_tx, read) = mpsc::channel();
            let source = Fixed {
                reading: MicReading::Supported(vec![whatsapp()]),
                read: read_tx,
            };
            let (started_tx, started) = mpsc::channel();
            let running = spawn_with_clock(
                source,
                Duration::from_millis(1),
                MicLoopHooks {
                    recording: move || recording,
                    never: move || never.clone(),
                    seen: |_: &MicReading| {},
                    started: move |call| {
                        let _ = started_tx.send(call);
                    },
                },
                stepping(),
            )
            .expect("spawns");
            wait_reads(&read, 20);
            running.stop();
            assert!(started.try_recv().is_err());
        }
    }

    #[test]
    fn not_supported_never_asks_and_is_still_seen() {
        let (read_tx, read) = mpsc::channel();
        let source = Fixed {
            reading: MicReading::NotSupported,
            read: read_tx,
        };
        let (seen_tx, seen) = mpsc::channel();
        let (started_tx, started) = mpsc::channel();
        let running = spawn_with_clock(
            source,
            Duration::from_millis(1),
            MicLoopHooks {
                recording: || false,
                never: Vec::new,
                seen: move |reading: &MicReading| {
                    let _ = seen_tx.send(reading.clone());
                },
                started: move |call| {
                    let _ = started_tx.send(call);
                },
            },
            stepping(),
        )
        .expect("spawns");
        wait_reads(&read, 20);
        running.stop();
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(10)).expect("seen"),
            MicReading::NotSupported
        );
        assert!(started.try_recv().is_err());
    }

    /// Nobody on the mic until `on` is set, then WhatsApp.
    struct Switching {
        on: Arc<std::sync::atomic::AtomicBool>,
        read: mpsc::Sender<()>,
    }

    impl MicSource for Switching {
        fn read(&mut self) -> MicReading {
            let _ = self.read.send(());
            if self.on.load(std::sync::atomic::Ordering::SeqCst) {
                MicReading::Supported(vec![whatsapp()])
            } else {
                MicReading::Supported(Vec::new())
            }
        }
    }

    #[test]
    fn not_now_reaches_the_rule() {
        let (read_tx, read) = mpsc::channel();
        let on = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let source = Switching {
            on: Arc::clone(&on),
            read: read_tx,
        };
        let (started_tx, started) = mpsc::channel();
        let running = spawn_with_clock(
            source,
            Duration::from_millis(1),
            MicLoopHooks {
                recording: || false,
                never: Vec::new,
                seen: |_: &MicReading| {},
                started: move |call| {
                    let _ = started_tx.send(call);
                },
            },
            stepping(),
        )
        .expect("spawns");
        wait_reads(&read, 1);
        running.dismissed("WhatsApp");
        // Reads after the message: the rule has it.
        wait_reads(&read, 2);
        on.store(true, std::sync::atomic::Ordering::SeqCst);
        // 40 s of WhatsApp on the mic on the loop's clock, well past the hold.
        wait_reads(&read, 20);
        running.stop();
        assert!(started.try_recv().is_err(), "Not now keeps it quiet");
    }
}
