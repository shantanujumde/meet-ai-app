//! Riding out a failed checkpoint (TUR-162).
//!
//! One failed `fsync`, `segments.json` rename or header patch used to end the
//! recording, though the checkpoint before it was still whole on disk: a busy
//! external disk, an iCloud folder or an antivirus lock on Windows can each
//! refuse one write and take the next. Now a failure is logged and the
//! checkpoint is tried again at the next tick; only a disk that keeps failing
//! ends the recording.
//!
//! "Keeps failing" is time only (TUR-163): TUR-162 also gave up after six
//! failures in a row, but the retry runs at every tick
//! ([`super::TICK_INTERVAL`], 200 ms), so six fast failures were about a
//! second of a busy disk.

use std::time::Duration;

/// How long after the last good checkpoint a failing one ends the recording:
/// about six checkpoint intervals of audio that no `segments.json` covers.
pub(super) const MAX_SINCE_GOOD: Duration = Duration::from_secs(30);

/// The run of failed checkpoints since the last good one.
#[derive(Debug, Default)]
pub(super) struct CheckpointRetry {
    failures: u32,
}

impl CheckpointRetry {
    /// A checkpoint (or a segment boundary) reached the disk.
    pub(super) fn succeeded(&mut self) {
        self.failures = 0;
    }

    /// Count a failed checkpoint, `since_good` after the last good one.
    /// `Err` means give up: [`MAX_SINCE_GOOD`] reached.
    pub(super) fn failed(&mut self, error: String, since_good: Duration) -> Result<(), String> {
        self.failures += 1;
        tracing::warn!(
            failures = self.failures,
            since_good_s = since_good.as_secs(),
            "checkpoint failed ({error}); the previous one on disk still holds, \
             trying again at the next tick"
        );
        if since_good >= MAX_SINCE_GOOD {
            return Err(format!(
                "{error} ({} checkpoints in a row failed; the last good one was {} s ago)",
                self.failures,
                since_good.as_secs()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOON: Duration = Duration::from_secs(5);

    #[test]
    fn failures_are_ridden_out_and_a_success_clears_them() {
        let mut retry = CheckpointRetry::default();
        for _ in 0..20 {
            retry.failed("fsync".into(), SOON).expect("not yet");
        }
        retry.succeeded();
        assert_eq!(retry.failures, 0);
    }

    /// TUR-163: a run of fast failures (one per 200 ms tick) no longer ends
    /// the recording after about a second; only the time does.
    #[test]
    fn many_failures_inside_thirty_seconds_never_give_up() {
        let mut retry = CheckpointRetry::default();
        for tick in 0..120u64 {
            let since_good = SOON + Duration::from_millis(200 * tick);
            retry
                .failed("fsync".into(), since_good)
                .unwrap_or_else(|e| panic!("gave up at {since_good:?}: {e}"));
        }
    }

    #[test]
    fn thirty_seconds_without_a_good_checkpoint_gives_up() {
        let mut retry = CheckpointRetry::default();
        let error = retry.failed("rename".into(), MAX_SINCE_GOOD).unwrap_err();
        assert!(error.starts_with("rename"), "{error}");
        assert!(error.contains("30 s ago"), "{error}");
    }
}
