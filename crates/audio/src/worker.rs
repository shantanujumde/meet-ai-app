//! A capture source's worker thread, stopped and joined when it is dropped
//! (TUR-162).
//!
//! Every source runs a worker that drains its ring into the WAV until a
//! shared `running` flag goes false. Before this, a source that was dropped
//! without an explicit `stop` (a microphone or tap whose start timed out and
//! finished later, or one a caller simply let go of) left that worker
//! polling an abandoned ring every couple of milliseconds until the app quit.
//! Holding the thread in a [`Worker`] makes the stop part of the drop.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

/// A running worker thread and the flag that keeps it running. Dropping it
/// clears the flag and joins the thread, so the thread never outlives it.
pub(crate) struct Worker {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Spawn a thread called `name` running `body`, which must return soon
    /// after the flag it is handed goes false.
    pub(crate) fn spawn(
        name: &str,
        body: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
    ) -> io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&running);
        let handle = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || body(flag))?;
        Ok(Self {
            running,
            handle: Some(handle),
        })
    }

    /// Clear the flag and wait for the thread to finish. Idempotent.
    pub(crate) fn halt(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            // A panicked worker has already stopped; nothing left to wait for.
            let _ = handle.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.halt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A worker that loops until told to stop, like the sources' own, and
    /// sets `exited` on its way out.
    fn looping(exited: &Arc<AtomicBool>) -> Worker {
        let exited = Arc::clone(exited);
        Worker::spawn("test-worker", move |running| {
            while running.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            exited.store(true, Ordering::Release);
        })
        .unwrap()
    }

    #[test]
    fn dropping_a_worker_stops_and_joins_its_thread() {
        let exited = Arc::new(AtomicBool::new(false));
        let worker = looping(&exited);
        drop(worker);
        assert!(
            exited.load(Ordering::Acquire),
            "the thread had finished by the time the drop returned"
        );
    }

    #[test]
    fn halting_twice_is_harmless() {
        let exited = Arc::new(AtomicBool::new(false));
        let mut worker = looping(&exited);
        worker.halt();
        assert!(exited.load(Ordering::Acquire));
        worker.halt();
        drop(worker);
    }
}
