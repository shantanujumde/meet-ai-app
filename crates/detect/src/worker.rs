//! The thread-and-stop handle both detection loops run on: a named thread
//! that does one step at once, then one more every interval or as soon as a
//! message arrives, until it is stopped or its handle is dropped.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

/// A running loop thread. Dropping it stops the loop and waits for it.
pub(crate) struct Worker<M: Send + 'static> {
    /// `None` asks the thread to stop.
    tx: mpsc::Sender<Option<M>>,
    thread: Option<JoinHandle<()>>,
}

impl<M: Send + 'static> Worker<M> {
    /// Run `step` on a thread called `name`: once at once with `None`, then
    /// again every `interval` (with `None`) or as soon as a message is sent
    /// (with that message).
    pub(crate) fn spawn<F>(name: &str, interval: Duration, mut step: F) -> std::io::Result<Self>
    where
        F: FnMut(Option<M>) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let mut message = None;
                loop {
                    step(message.take());
                    match rx.recv_timeout(interval) {
                        Err(RecvTimeoutError::Timeout) => {}
                        Ok(Some(next)) => message = Some(next),
                        Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
        Ok(Self {
            tx,
            thread: Some(thread),
        })
    }

    /// Hand `message` to the next step, which runs at once.
    pub(crate) fn send(&self, message: M) {
        let _ = self.tx.send(Some(message));
    }

    /// Stop the loop and wait for its thread.
    pub(crate) fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        let _ = self.tx.send(None);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl<M: Send + 'static> Drop for Worker<M> {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_at_once_then_on_each_message_and_stops() {
        let (seen_tx, seen) = mpsc::channel();
        let worker = Worker::spawn("test-worker", Duration::from_secs(3600), move |message| {
            let _ = seen_tx.send(message);
        })
        .expect("spawns");
        let wait = || seen.recv_timeout(Duration::from_secs(10)).expect("stepped");
        assert_eq!(wait(), None);
        worker.send(7);
        assert_eq!(wait(), Some(7));
        worker.stop();
        assert!(seen.try_recv().is_err());
    }

    #[test]
    fn dropping_the_handle_stops_the_thread() {
        let (seen_tx, seen) = mpsc::channel::<Option<u8>>();
        let worker = Worker::spawn("test-worker", Duration::from_millis(1), move |message| {
            let _ = seen_tx.send(message);
        })
        .expect("spawns");
        seen.recv_timeout(Duration::from_secs(10)).expect("stepped");
        drop(worker);
        // The thread has exited and dropped its sender.
        while seen.try_recv().is_ok() {}
        assert!(matches!(
            seen.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
