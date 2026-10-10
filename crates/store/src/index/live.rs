//! Throttling the meeting being recorded (TUR-166).
//!
//! `transcript.md` grows by a line every few seconds while a meeting records,
//! so every watcher batch (about 500 ms apart) named that meeting, and each
//! one re-read the whole transcript and rewrote all its rows. Search over a
//! meeting still being recorded can lag a little, so its changes are indexed
//! at most once per [`LIVE_REINDEX_EVERY`], and once more, in full, when the
//! recording stops.

use std::path::Path;
use std::time::{Duration, Instant};

use super::{Index, index_error, reindex};
use crate::Error;

/// How often, at most, the meeting being recorded is indexed again.
pub const LIVE_REINDEX_EVERY: Duration = Duration::from_secs(10);

/// Which meeting is recording, and when it was last let through.
#[derive(Debug, Default)]
pub(super) struct LiveThrottle {
    id: Option<String>,
    last: Option<Instant>,
}

impl LiveThrottle {
    /// `live` is now the meeting being recorded (`None`: nothing is). Returns
    /// the meeting that stopped recording, if one did, to be indexed again.
    fn set(&mut self, live: Option<&str>) -> Option<String> {
        if self.id.as_deref() == live {
            return None;
        }
        self.last = None;
        std::mem::replace(&mut self.id, live.map(str::to_owned))
    }

    /// Whether a change to meeting `id` is indexed now. Any meeting but the
    /// live one always is; the live one at most once per
    /// [`LIVE_REINDEX_EVERY`], the first change straight away.
    pub(super) fn admit(&mut self, id: &str, now: Instant) -> bool {
        if self.id.as_deref() != Some(id) {
            return true;
        }
        let due = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= LIVE_REINDEX_EVERY);
        if due {
            self.last = Some(now);
        }
        due
    }
}

impl Index {
    /// Tell the index which meeting is being recorded (`None`: none is).
    /// When that changes, the meeting that stopped is indexed again now if
    /// its files changed since its rows were written, so the changes held
    /// back while it recorded are not lost. Returns how many meetings that
    /// re-indexed (0 or 1).
    ///
    /// # Errors
    ///
    /// [`Error::Index`] when the database cannot be written.
    pub fn set_live(&mut self, root: &Path, live: Option<&str>) -> Result<usize, Error> {
        let Some(stopped) = self.live.set(live) else {
            return Ok(0);
        };
        if !crate::is_plain_name(&stopped) {
            return Ok(0);
        }
        let tx = self.conn.transaction().map_err(index_error)?;
        let changed = reindex(&tx, root, &stopped, false)?;
        tx.commit().map_err(index_error)?;
        Ok(usize::from(changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_meeting_is_let_through_once_per_interval() {
        let mut throttle = LiveThrottle::default();
        let start = Instant::now();
        assert!(throttle.admit("a", start), "nothing is live yet");
        assert_eq!(throttle.set(Some("a")), None);
        assert!(
            throttle.admit("a", start),
            "the first change goes straight in"
        );
        assert!(!throttle.admit("a", start + Duration::from_millis(500)));
        assert!(throttle.admit("b", start + Duration::from_millis(500)));
        assert!(!throttle.admit("a", start + Duration::from_secs(9)));
        assert!(throttle.admit("a", start + LIVE_REINDEX_EVERY));
        assert!(!throttle.admit("a", start + LIVE_REINDEX_EVERY));
    }

    #[test]
    fn stopping_names_the_meeting_that_stopped() {
        let mut throttle = LiveThrottle::default();
        assert_eq!(throttle.set(None), None);
        assert_eq!(throttle.set(Some("a")), None);
        assert_eq!(throttle.set(Some("a")), None, "no change");
        assert_eq!(throttle.set(Some("b")).as_deref(), Some("a"));
        assert_eq!(throttle.set(None).as_deref(), Some("b"));
        assert!(throttle.admit("b", Instant::now()));
    }
}
