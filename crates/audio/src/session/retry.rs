//! Riding out a failed checkpoint (TUR-162).
//!
//! One failed `fsync`, `segments.json` rename or header patch used to end the
//! recording, though the checkpoint before it was still whole on disk: a busy
//! external disk, an iCloud folder or an antivirus lock on Windows can each
//! refuse one write and take the next. Now a failure is logged and the
//! checkpoint is tried again at the next tick; only a disk that keeps failing
//! ends the recording.

use std::time::Duration;

/// Failed checkpoints in a row that end the recording. The retry runs at
/// every tick ([`super::TICK_INTERVAL`]), so failures that come back at once
/// use these up in about a second, and ones that each block for seconds run
/// into [`MAX_SINCE_GOOD`] first.
pub(super) const MAX_CONSECUTIVE_FAILURES: u32 = 6;

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
    /// `Err` means give up: [`MAX_CONSECUTIVE_FAILURES`] in a row, or
    /// [`MAX_SINCE_GOOD`] reached, whichever comes first.
    pub(super) fn failed(&mut self, error: String, since_good: Duration) -> Result<(), String> {
        self.failures += 1;
        tracing::warn!(
            failures = self.failures,
            since_good_s = since_good.as_secs(),
            "checkpoint failed ({error}); the previous one on disk still holds, \
             trying again at the next tick"
        );
        if self.failures >= MAX_CONSECUTIVE_FAILURES || since_good >= MAX_SINCE_GOOD {
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
    fn a_few_failures_are_ridden_out_and_a_success_clears_them() {
        let mut retry = CheckpointRetry::default();
        for _ in 1..MAX_CONSECUTIVE_FAILURES {
            retry.failed("fsync".into(), SOON).expect("not yet");
        }
        retry.succeeded();
        for _ in 1..MAX_CONSECUTIVE_FAILURES {
            retry.failed("fsync".into(), SOON).expect("counted afresh");
        }
    }

    #[test]
    fn the_sixth_failure_in_a_row_gives_up() {
        let mut retry = CheckpointRetry::default();
        for _ in 1..MAX_CONSECUTIVE_FAILURES {
            retry.failed("fsync".into(), SOON).unwrap();
        }
        let error = retry.failed("fsync".into(), SOON).unwrap_err();
        assert!(error.starts_with("fsync"), "{error}");
        assert!(error.contains("6 checkpoints in a row"), "{error}");
    }

    #[test]
    fn thirty_seconds_without_a_good_checkpoint_gives_up_at_once() {
        let mut retry = CheckpointRetry::default();
        assert!(retry.failed("rename".into(), MAX_SINCE_GOOD).is_err());
    }
}
