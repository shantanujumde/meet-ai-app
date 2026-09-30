//! The worker thread that drives `RecordingSession::tick` for as long as a
//! recording is live (TUR-97).
//!
//! **Why this exists.** `audio::session::RecordingSession` does not tick
//! itself: its caller owns the cadence (see that type's docs), and `tick()` is
//! the only thing that runs the SPEC §3.4 / contract §7 checkpoint — fsync
//! both WAVs, write `segments.json` with an anchor, patch both headers — and
//! the SPEC §5 default-device check that reopens a segment on an AirPods swap.
//! `meet-rec` calls it in its main loop; the app, up to v0.3.0, never did. So
//! a recording that ended any way other than a clean stop (`kill -9`, a
//! crash, a power cut) left WAVs whose headers declared 0 bytes over ~16 s of
//! real PCM and no `segments.json` at all — measured on the installed 0.3.0
//! build — and the AirPods-swap reopen shipped in 0.2.0 never ran in the app.
//!
//! **Why the thread owns the state rather than locking for it.** The obvious
//! shape — keep the session in `Recorder`'s `Mutex` and have this thread lock,
//! tick, unlock — holds that lock for the whole of a tick. Most ticks are two
//! Core Audio property reads, but a checkpoint is three fsyncs, and a segment
//! reopen restarts both channels and waits up to `FIRST_BUFFER_TIMEOUT` (10 s)
//! per channel for first audio. `Recorder::status()` is called from the main
//! thread (the tray's `recording://state` listener, the sync
//! `recording_status` command), so a lock held through a reopen would freeze
//! the whole UI for seconds. Instead the thread takes the state by value and
//! is the only thing that touches it while it runs; [`Ticker::stop`] gets it
//! back through the join. That makes the two guarantees the stop path needs
//! structural rather than a matter of lock ordering:
//!
//! - **No tick after stop.** The state comes back only from the thread's
//!   return value, so once [`Ticker::stop`] has it, the thread has exited and
//!   no tick can be running or start later.
//! - **Join before the session's own `stop()`.** The caller cannot call
//!   `RecordingSession::stop` without the session, and the only way to get it
//!   is to finish the join first.
//!
//! Generic over the state and the tick closure so the start/stop/join logic is
//! testable here without an `AppHandle`, Core Audio or a microphone grant; the
//! recording-specific parts (what a failed tick means) live in `recording.rs`.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

/// A named worker thread that calls `tick` on some state every `interval`
/// until told to stop, and hands the state back when it is.
///
/// Deliberately no `Drop` that joins: the failure path runs *on* this thread
/// and has to be able to let go of its own handle ([`Ticker::detach`]), and a
/// thread joining itself deadlocks.
pub(super) struct Ticker<T> {
    /// Sending on (or dropping) this wakes the thread out of its wait at once,
    /// so stopping never waits out the rest of an interval.
    stop: mpsc::Sender<()>,
    thread: JoinHandle<Option<T>>,
}

impl<T: Send + 'static> Ticker<T> {
    /// Start the thread and move `state` onto it.
    ///
    /// The thread waits `interval`, calls `tick(&mut state)`, and repeats —
    /// wait first, then tick, the same order as `meet-rec`'s loop. When a tick
    /// returns `Err`, the loop ends and `on_fail(state, error)` runs on this
    /// thread; whatever it returns is what a later [`Ticker::stop`] gets back.
    ///
    /// On a spawn failure the state is handed back with the error rather than
    /// dropped with the closure, so a caller holding a live `RecordingSession`
    /// can still stop it cleanly instead of losing its headers. That is why
    /// the state crosses to the thread over a channel after the spawn instead
    /// of being captured by it.
    pub(super) fn spawn<E, F, G>(
        name: &str,
        interval: Duration,
        state: T,
        mut tick: F,
        on_fail: G,
    ) -> Result<Self, (T, std::io::Error)>
    where
        E: Send + 'static,
        F: FnMut(&mut T) -> Result<(), E> + Send + 'static,
        G: FnOnce(T, E) -> Option<T> + Send + 'static,
    {
        let (stop, stopped) = mpsc::channel::<()>();
        let (hand_over, handed) = mpsc::sync_channel::<T>(1);

        let spawned = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                // Only fails if the spawner dropped the sender without sending,
                // which it never does; returning `None` is just the total
                // answer.
                let Ok(mut state) = handed.recv() else {
                    return None;
                };
                loop {
                    match stopped.recv_timeout(interval) {
                        Err(RecvTimeoutError::Timeout) => {}
                        // An explicit stop, or the handle dropped by
                        // `detach`: either way nobody wants more ticks.
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => return Some(state),
                    }
                    if let Err(error) = tick(&mut state) {
                        return on_fail(state, error);
                    }
                }
            });

        let thread = match spawned {
            Ok(thread) => thread,
            Err(error) => return Err((state, error)),
        };
        // The receiver is alive: the thread's first act is to block on it, and
        // nothing before that can panic. Still, never drop the state silently.
        if let Err(mpsc::SendError(state)) = hand_over.send(state) {
            return Err((
                state,
                std::io::Error::other("the ticker thread exited before it was handed its state"),
            ));
        }
        Ok(Self { stop, thread })
    }

    /// Stop ticking, wait for the thread to exit, and take the state back.
    ///
    /// Returns promptly: the thread is woken out of its wait at once, so the
    /// only delay is a tick already in flight, which has to finish before the
    /// state can be handed back anyway. `Ok(None)` means a failed tick's
    /// `on_fail` kept the state; `Err` means the thread panicked and the state
    /// went with it.
    ///
    /// Must not be called from the ticker's own thread (a self-join
    /// deadlocks); a failure on that thread uses [`Ticker::detach`] instead.
    pub(super) fn stop(self) -> Result<Option<T>, String> {
        // A send error only means the thread has already left its loop
        // (a failed tick), which is exactly the case where there is nothing
        // to wake.
        let _ = self.stop.send(());
        self.thread
            .join()
            .map_err(|payload| format!("the ticker thread panicked: {}", panic_message(&*payload)))
    }

    /// Let go of the thread without joining it. For the failure path, which
    /// runs on the ticker thread itself and is about to return from it.
    pub(super) fn detach(self) {
        drop(self);
    }
}

/// The text of a panic payload, for a log line or an error message.
pub(super) fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "no message"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;

    const FAST: Duration = Duration::from_millis(5);

    /// Poll `done` until it holds or two seconds pass. Generous so a loaded CI
    /// machine does not turn scheduling jitter into a failure.
    fn eventually(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        done()
    }

    fn never_fails(state: u32, _error: ()) -> Option<u32> {
        Some(state)
    }

    #[test]
    fn ticks_keep_happening_while_the_ticker_runs() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let ticker = Ticker::spawn(
            "ticker-test-runs",
            FAST,
            0u32,
            move |state: &mut u32| {
                *state += 1;
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<(), ()>(())
            },
            never_fails,
        )
        .expect("spawns");

        assert!(
            eventually(|| seen.load(Ordering::SeqCst) >= 3),
            "a running ticker must keep calling tick on its interval"
        );
        let state = ticker
            .stop()
            .expect("joins cleanly")
            .expect("state comes back");
        assert!(state >= 3);
    }

    /// The TUR-97 guarantee the stop path leans on: once `stop` has returned,
    /// the thread is gone — nothing ticks the state again, and the state the
    /// caller got back already includes every tick that ran.
    #[test]
    fn no_tick_runs_after_stop_returns() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let ticker = Ticker::spawn(
            "ticker-test-after-stop",
            Duration::from_millis(1),
            0u32,
            move |state: &mut u32| {
                *state += 1;
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<(), ()>(())
            },
            never_fails,
        )
        .expect("spawns");
        assert!(eventually(|| seen.load(Ordering::SeqCst) >= 2));

        let state = ticker
            .stop()
            .expect("joins cleanly")
            .expect("state comes back");
        let at_stop = seen.load(Ordering::SeqCst);
        assert_eq!(
            state as usize, at_stop,
            "the returned state must reflect every tick that ran, i.e. the thread has exited"
        );

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            seen.load(Ordering::SeqCst),
            at_stop,
            "no tick may run after stop has returned"
        );
    }

    /// Stop waits for a tick already in flight rather than handing the state
    /// back mid-tick — the ordering that keeps a checkpoint from racing
    /// `RecordingSession::stop`.
    #[test]
    fn stop_waits_for_an_in_flight_tick_to_finish() {
        let in_tick = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicUsize::new(0));
        let (flag, done) = (Arc::clone(&in_tick), Arc::clone(&finished));
        let ticker = Ticker::spawn(
            "ticker-test-in-flight",
            Duration::from_millis(1),
            0u32,
            move |state: &mut u32| {
                flag.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(100));
                *state += 1;
                done.fetch_add(1, Ordering::SeqCst);
                flag.store(false, Ordering::SeqCst);
                Ok::<(), ()>(())
            },
            never_fails,
        )
        .expect("spawns");
        assert!(eventually(|| in_tick.load(Ordering::SeqCst)));

        let state = ticker
            .stop()
            .expect("joins cleanly")
            .expect("state comes back");
        assert!(
            !in_tick.load(Ordering::SeqCst),
            "stop must not return while a tick is still running"
        );
        assert!(
            state >= 1,
            "the in-flight tick completed before the hand-back"
        );
        assert_eq!(state as usize, finished.load(Ordering::SeqCst));
    }

    /// Stopping must not wait out the rest of the interval: ⌘⇧R to stop should
    /// feel instant, not up to one poll late.
    #[test]
    fn stop_wakes_the_thread_instead_of_waiting_out_the_interval() {
        let ticker = Ticker::spawn(
            "ticker-test-wake",
            Duration::from_secs(60),
            0u32,
            |state: &mut u32| {
                *state += 1;
                Ok::<(), ()>(())
            },
            never_fails,
        )
        .expect("spawns");

        let asked = Instant::now();
        let state = ticker
            .stop()
            .expect("joins cleanly")
            .expect("state comes back");
        assert!(asked.elapsed() < Duration::from_secs(5));
        assert_eq!(state, 0, "no interval elapsed, so no tick ran");
    }

    /// A failed tick ends the loop and hands the state and the error to
    /// `on_fail` on the ticker thread — no further ticks after the failure.
    #[test]
    fn a_failing_tick_ends_the_loop_and_hands_the_state_to_on_fail() {
        let seen = Arc::new(AtomicUsize::new(0));
        let failed_with = Arc::new(Mutex::new(None));
        let (counter, record) = (Arc::clone(&seen), Arc::clone(&failed_with));
        let ticker = Ticker::spawn(
            "ticker-test-fail",
            Duration::from_millis(1),
            0u32,
            move |state: &mut u32| {
                *state += 1;
                counter.fetch_add(1, Ordering::SeqCst);
                if *state == 3 {
                    Err("disk full")
                } else {
                    Ok(())
                }
            },
            move |state, error| {
                *record.lock().unwrap() = Some((state, error));
                None
            },
        )
        .expect("spawns");

        assert!(eventually(|| failed_with.lock().unwrap().is_some()));
        assert_eq!(*failed_with.lock().unwrap(), Some((3, "disk full")));

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(seen.load(Ordering::SeqCst), 3, "no tick after a failure");
        assert_eq!(
            ticker.stop().expect("joins cleanly"),
            None,
            "on_fail kept the state, so a later stop gets nothing back"
        );
    }

    /// `on_fail` may hand the state back instead — the case where a user's
    /// stop has already claimed the recording and is joining this thread.
    #[test]
    fn on_fail_can_hand_the_state_back_to_the_joiner() {
        let ticker = Ticker::spawn(
            "ticker-test-hand-back",
            Duration::from_millis(1),
            7u32,
            |_state: &mut u32| Err(()),
            |state, ()| Some(state),
        )
        .expect("spawns");
        assert_eq!(ticker.stop().expect("joins cleanly"), Some(7));
    }

    #[test]
    fn a_panicking_tick_is_reported_by_stop_not_swallowed() {
        let ticker = Ticker::spawn(
            "ticker-test-panic",
            Duration::from_millis(1),
            0u32,
            |_state: &mut u32| -> Result<(), ()> { panic!("boom") },
            never_fails,
        )
        .expect("spawns");
        std::thread::sleep(Duration::from_millis(20));
        let error = ticker
            .stop()
            .expect_err("a panicked thread cannot return state");
        assert!(error.contains("boom"), "{error}");
    }
}
