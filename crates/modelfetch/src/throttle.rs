//! Fewer progress reports (TUR-159).
//!
//! The network hands over a model in tens of thousands of chunks, and the app
//! forwarded each one to the window as a Tauri event. A bar only needs to move
//! when the percentage it shows changes, plus a steady tick so the byte count
//! keeps moving on a slow connection where one percent takes a while.

use std::time::{Duration, Instant};

use crate::Progress;

/// The longest gap between two reports while bytes are arriving.
pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Decides which progress reports reach the caller.
#[derive(Debug, Default)]
pub(crate) struct Throttle {
    last: Option<(u64, Instant)>,
}

impl Throttle {
    /// Should `progress`, seen at `now`, be reported?
    ///
    /// Yes for the first report, for every verifying one (each marks a change
    /// of state, and there are only one or two per file), when the whole
    /// percentage differs from the last one reported, and when
    /// [`PROGRESS_INTERVAL`] has passed since then.
    pub(crate) fn admit(&mut self, progress: &Progress, now: Instant) -> bool {
        let percent = (progress.fraction() * 100.0) as u64;
        let due = match self.last {
            None => true,
            Some((last_percent, at)) => {
                progress.verifying
                    || percent != last_percent
                    || now.saturating_duration_since(at) >= PROGRESS_INTERVAL
            }
        };
        if due {
            self.last = Some((percent, now));
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(downloaded_bytes: u64) -> Progress {
        Progress {
            downloaded_bytes,
            total_bytes: 1_000,
            verifying: false,
        }
    }

    #[test]
    fn only_a_new_percentage_or_a_tick_gets_through() {
        let start = Instant::now();
        let mut throttle = Throttle::default();
        assert!(throttle.admit(&at(0), start), "the first report");
        // 0.1% steps inside the same whole percent, close together: dropped.
        assert!(!throttle.admit(&at(1), start));
        assert!(!throttle.admit(&at(9), start + Duration::from_millis(50)));
        // The next whole percent.
        assert!(throttle.admit(&at(10), start + Duration::from_millis(60)));
        // Same percent, but a tick has passed since the last report.
        assert!(!throttle.admit(&at(11), start + Duration::from_millis(159)));
        assert!(throttle.admit(&at(12), start + Duration::from_millis(160)));
    }

    #[test]
    fn verifying_is_always_reported() {
        let now = Instant::now();
        let mut throttle = Throttle::default();
        assert!(throttle.admit(&at(1_000), now));
        let verifying = Progress {
            verifying: true,
            ..at(1_000)
        };
        assert!(throttle.admit(&verifying, now));
    }

    #[test]
    fn ten_thousand_chunks_become_about_a_hundred_reports() {
        let start = Instant::now();
        let mut throttle = Throttle::default();
        let total = 10_000u64;
        let reported = (1..=total)
            .filter(|&chunk| {
                let progress = Progress {
                    downloaded_bytes: chunk,
                    total_bytes: total,
                    verifying: false,
                };
                // All within one tick: only percentage changes count.
                throttle.admit(&progress, start + Duration::from_micros(chunk))
            })
            .count();
        assert!(reported <= 101, "{reported} reports");
    }
}
