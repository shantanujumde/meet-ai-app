//! The `detection` section as the running loops see it (TUR-78).
//!
//! Settings → Notifications changes the switches and the lead time while the
//! app runs, and the loops never restart: each one asks [`Live::get`] on its
//! own tick (the process list every 5 s, the devices every 2 s, the reminder
//! every 10 s). Reading `config.jsonc` that often for three loops is wasted
//! work, so the answer is kept for [`REREAD_AFTER`]; a save from Settings
//! hands the new section over at once with [`Live::set`], and an edit by hand
//! is seen within that time.
//!
//! The read itself happens with the lock released (TUR-172): a slow disk
//! then holds up only the loop that is reading, never the other two, and
//! never a save from Settings.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::DetectionConfig;
use crate::lock::lock_or_recover;

/// How long a read of `config.jsonc` is trusted before reading it again.
pub const REREAD_AFTER: Duration = Duration::from_secs(5);

type Reader = Box<dyn Fn() -> DetectionConfig + Send + Sync>;

/// The latest `detection` section, re-read at most every [`REREAD_AFTER`].
pub struct Live {
    read: Reader,
    latest: Mutex<Option<(DetectionConfig, Instant)>>,
}

impl Default for Live {
    /// Reading `~/Meetings/.app/config.jsonc` ([`crate::config::detection`]).
    fn default() -> Self {
        Self::with_reader(crate::config::detection)
    }
}

impl Live {
    /// Reading through `read` instead of the real file, for tests.
    pub fn with_reader(read: impl Fn() -> DetectionConfig + Send + Sync + 'static) -> Self {
        Self {
            read: Box::new(read),
            latest: Mutex::new(None),
        }
    }

    /// The section now: the last one read or set, unless that is older than
    /// [`REREAD_AFTER`].
    pub fn get(&self) -> DetectionConfig {
        self.get_at(Instant::now())
    }

    fn get_at(&self, now: Instant) -> DetectionConfig {
        let seen = match *lock_or_recover(&self.latest) {
            Some((config, at)) if now.saturating_duration_since(at) < REREAD_AFTER => {
                return config;
            }
            stale => stale.map(|(_, at)| at),
        };
        let config = (self.read)();
        let mut latest = lock_or_recover(&self.latest);
        match *latest {
            // A save or another loop's read landed while this one read: it is
            // at least as new, so it wins.
            Some((newer, at)) if Some(at) != seen => newer,
            _ => {
                *latest = Some((config, now));
                config
            }
        }
    }

    /// Settings just saved `config`: every loop sees it from its next tick.
    pub fn set(&self, config: DetectionConfig) {
        *lock_or_recover(&self.latest) = Some((config, Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn counting() -> (Live, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reads);
        let live = Live::with_reader(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            DetectionConfig::default()
        });
        (live, reads)
    }

    #[test]
    fn reads_once_and_again_after_the_reread_time() {
        let (live, reads) = counting();
        let start = Instant::now();
        assert_eq!(live.get_at(start), DetectionConfig::default());
        live.get_at(start + Duration::from_secs(1));
        live.get_at(start + Duration::from_secs(4));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        live.get_at(start + REREAD_AFTER);
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_slow_read_holds_no_lock_and_a_save_during_it_wins() {
        let (started_tx, started) = std::sync::mpsc::channel();
        let (finish_tx, finish) = std::sync::mpsc::channel::<()>();
        let finish = std::sync::Mutex::new(finish);
        let live = Arc::new(Live::with_reader(move || {
            let _ = started_tx.send(());
            // The disk is slow: wait until the test says the read is done.
            let _ = finish.lock().unwrap().recv();
            DetectionConfig::default()
        }));
        let reader = Arc::clone(&live);
        let slow = std::thread::spawn(move || reader.get());
        started.recv().unwrap();

        // Mid-read: a save goes through instead of waiting on the disk.
        let off = DetectionConfig {
            processes: false,
            ..DetectionConfig::default()
        };
        live.set(off);
        finish_tx.send(()).unwrap();

        // The read that started before the save does not undo it.
        assert_eq!(slow.join().unwrap(), off);
        assert_eq!(live.get(), off);
    }

    #[test]
    fn a_save_applies_at_once_without_a_read() {
        let (live, reads) = counting();
        let off = DetectionConfig {
            processes: false,
            ..DetectionConfig::default()
        };
        live.set(off);
        assert_eq!(live.get(), off);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}
