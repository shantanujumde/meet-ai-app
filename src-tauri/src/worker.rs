//! A named thread that can be told to stop, and the text of a panic (TUR-176).
//!
//! The recording ticker (`recording::ticker`) and the meeting reminder loop
//! (`detection::reminder`) both run a loop on a thread of their own that
//! waits between rounds and must wake at once when told to stop. [`spawn`]
//! is that thread: its body gets a [`StopSignal`] to wait on, and
//! [`Worker::stop`] wakes it and joins it.

use std::any::Any;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

/// The text of a panic payload, for a log line or an error message.
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "no message"
    }
}

/// What a [`Worker`]'s body waits on between rounds.
pub(crate) struct StopSignal(mpsc::Receiver<()>);

impl StopSignal {
    /// Wait up to `interval`. `true` when it passed and the loop should go
    /// on; `false` at once on a stop, or when the [`Worker`] was dropped.
    pub(crate) fn wait(&self, interval: Duration) -> bool {
        matches!(
            self.0.recv_timeout(interval),
            Err(RecvTimeoutError::Timeout)
        )
    }
}

/// A running thread from [`spawn`].
///
/// No `Drop` that joins: a body that ends itself may have to let go of its
/// own handle, and a thread joining itself deadlocks. Dropping the handle
/// only tells the body to stop.
pub(crate) struct Worker<T> {
    /// Sending on (or dropping) this wakes the body out of its wait at once.
    stop: mpsc::Sender<()>,
    thread: JoinHandle<T>,
}

/// Start a thread called `name` running `body`, which waits on the
/// [`StopSignal`] it is given.
pub(crate) fn spawn<T, F>(name: &str, body: F) -> std::io::Result<Worker<T>>
where
    T: Send + 'static,
    F: FnOnce(StopSignal) -> T + Send + 'static,
{
    let (stop, stopped) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || body(StopSignal(stopped)))?;
    Ok(Worker { stop, thread })
}

impl<T> Worker<T> {
    /// Wake the body, wait for its thread to exit, and take what it returned.
    /// `Err` is the payload of a panic on it.
    ///
    /// Must not be called from the worker's own thread (a self-join
    /// deadlocks).
    pub(crate) fn stop(self) -> std::thread::Result<T> {
        // A send error only means the body has already stopped waiting,
        // which is exactly the case where there is nothing to wake.
        let _ = self.stop.send(());
        self.thread.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_wakes_the_body_and_hands_back_what_it_returned() {
        let worker = spawn("worker-test-stop", |stop| {
            let mut rounds = 0u32;
            while stop.wait(Duration::from_secs(60)) {
                rounds += 1;
            }
            rounds
        })
        .expect("spawns");
        assert_eq!(worker.stop().expect("joins cleanly"), 0);
    }

    #[test]
    fn a_dropped_worker_stops_waiting() {
        let (done, finished) = mpsc::channel();
        let worker = spawn("worker-test-drop", move |stop| {
            let waited = stop.wait(Duration::from_secs(60));
            let _ = done.send(waited);
        })
        .expect("spawns");
        drop(worker);
        let waited = finished
            .recv_timeout(Duration::from_secs(10))
            .expect("the body ended");
        assert!(!waited, "a dropped handle is a stop, not a passed interval");
    }

    #[test]
    fn a_passed_interval_keeps_the_loop_going() {
        let worker = spawn("worker-test-interval", |stop| {
            stop.wait(Duration::from_millis(1))
        })
        .expect("spawns");
        std::thread::sleep(Duration::from_millis(20));
        assert!(worker.stop().expect("joins cleanly"));
    }

    #[test]
    fn a_panic_message_is_the_text_or_a_fallback() {
        let text: Box<dyn Any + Send> = Box::new("boom");
        let owned: Box<dyn Any + Send> = Box::new(String::from("bang"));
        let other: Box<dyn Any + Send> = Box::new(3u8);
        assert_eq!(panic_message(&*text), "boom");
        assert_eq!(panic_message(&*owned), "bang");
        assert_eq!(panic_message(&*other), "no message");
    }
}
