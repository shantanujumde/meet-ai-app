//! When a default-device change is real (TUR-37).
//!
//! On Windows, `cpal` 0.18.2 opens default-device streams through the
//! virtual default endpoint, so the OS reroutes them to a new default by
//! itself (a headset plugged in); `cpal` only reports a `StreamInvalidated`
//! error and carries on. The recording still has to notice, because the
//! contract opens a new segment at every device change
//! (`reason::DEFAULT_OUTPUT_DEVICE_CHANGED`), and the new device's rate can
//! differ. The session polls `crate::platform::default_output_device` every
//! tick; on Windows that answer comes from a [`DeviceWatch`]: the endpoint
//! is read every [`DEVICE_CHECK_INTERVAL`], and a new one only counts once
//! [`DEVICE_SWITCH_CONFIRMATIONS`] reads in a row agree, so a headset that
//! flaps between its two endpoints while connecting opens one segment, not
//! three.
//!
//! Pure, so it is tested on every OS with a fake clock and fake reads.

use std::time::Duration;

/// How often the endpoint is read.
pub const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// How many reads in a row must name the same new endpoint before it counts.
pub const DEVICE_SWITCH_CONFIRMATIONS: u8 = 2;

/// A watch not read for this long (no recording polled it) starts over from
/// whatever the next read says, rather than taking the first read of a new
/// recording as a switch.
pub const STALE_AFTER: Duration = Duration::from_secs(6);

// Adapted from github.com/fastrepl/anarlog/crates/audio-actual/src/speaker/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// Which endpoint is being followed, and the switch that may be under way:
/// [`Self::observe`] reports a switch once the reads have stayed away from
/// the followed endpoint for [`DEVICE_SWITCH_CONFIRMATIONS`] reads in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointFollower {
    current: String,
    pending: Option<(String, u8)>,
}

impl EndpointFollower {
    pub fn new(current: String) -> Self {
        Self {
            current,
            pending: None,
        }
    }

    /// The endpoint being followed.
    pub fn current(&self) -> &str {
        &self.current
    }

    /// One read. Returns the new endpoint once the switch to it is confirmed.
    pub fn observe(&mut self, observed: String) -> Option<String> {
        if observed == self.current {
            self.pending = None;
            return None;
        }

        let count = match &self.pending {
            Some((id, count)) if *id == observed => count.saturating_add(1),
            _ => 1,
        };

        if count >= DEVICE_SWITCH_CONFIRMATIONS {
            self.pending = None;
            Some(observed)
        } else {
            self.pending = Some((observed, count));
            None
        }
    }

    /// Forget a switch under way (a read failed).
    pub fn reset(&mut self) {
        self.pending = None;
    }
}

/// [`EndpointFollower`] on a timer: what `default_output_device` (and the
/// input one) answers on Windows.
#[derive(Debug, Default)]
pub struct DeviceWatch {
    follower: Option<EndpointFollower>,
    /// When the endpoint was last read, on the caller's clock.
    last_read_ns: Option<u64>,
}

impl DeviceWatch {
    /// A watch that has read nothing yet; `const` so it can be a `static`.
    pub const fn new() -> Self {
        Self {
            follower: None,
            last_read_ns: None,
        }
    }

    /// The endpoint to report now, on a monotonic clock `now_ns`. `read`
    /// reads the endpoint's id from the OS (`None`: no default endpoint, or
    /// the read failed) and is only called when a read is due:
    ///
    /// * the first time, or after [`STALE_AFTER`] without a poll: the read is
    ///   taken as is;
    /// * otherwise every [`DEVICE_CHECK_INTERVAL`]: the read goes through the
    ///   [`EndpointFollower`]. A failed read keeps the endpoint followed so
    ///   far (one bad read is not a switch) and cancels a switch under way.
    ///
    /// `None` until some read has succeeded.
    pub fn poll(&mut self, now_ns: u64, read: impl FnOnce() -> Option<String>) -> Option<String> {
        let since = self.last_read_ns.map(|last| now_ns.saturating_sub(last));
        let stale = since.is_none_or(|ns| ns >= STALE_AFTER.as_nanos() as u64);
        let due = since.is_none_or(|ns| ns >= DEVICE_CHECK_INTERVAL.as_nanos() as u64);
        if stale || (self.follower.is_none() && due) {
            self.last_read_ns = Some(now_ns);
            self.follower = read().map(EndpointFollower::new);
        } else if due {
            self.last_read_ns = Some(now_ns);
            if let Some(follower) = self.follower.as_mut() {
                match read() {
                    Some(observed) => {
                        if let Some(next) = follower.observe(observed) {
                            *follower = EndpointFollower::new(next);
                        }
                    }
                    None => follower.reset(),
                }
            }
        }
        self.follower.as_ref().map(|f| f.current().to_string())
    }
}

/// A `Copy` key for an endpoint id string, which is what the session compares
/// (`crate::platform::DeviceId`). FNV-1a: stable across runs and builds,
/// unlike `std`'s hasher.
pub fn endpoint_key(id: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    id.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    // The four follower tests are anarlog's, as they were.
    #[test]
    fn endpoint_follower_switches_after_consecutive_confirmations() {
        let mut follower = EndpointFollower::new("speakers".into());
        assert_eq!(follower.observe("headset".into()), None);
        assert_eq!(
            follower.observe("headset".into()),
            Some("headset".to_string())
        );
    }

    #[test]
    fn endpoint_follower_resets_on_one_off_change() {
        let mut follower = EndpointFollower::new("speakers".into());
        assert_eq!(follower.observe("headset".into()), None);
        assert_eq!(follower.observe("speakers".into()), None);
        assert_eq!(follower.observe("headset".into()), None);
        assert_eq!(
            follower.observe("headset".into()),
            Some("headset".to_string())
        );
    }

    #[test]
    fn endpoint_follower_restarts_count_when_candidate_changes() {
        let mut follower = EndpointFollower::new("speakers".into());
        assert_eq!(follower.observe("headset".into()), None);
        assert_eq!(follower.observe("monitor".into()), None);
        assert_eq!(
            follower.observe("monitor".into()),
            Some("monitor".to_string())
        );
    }

    #[test]
    fn endpoint_follower_reset_clears_partial_confirmation() {
        let mut follower = EndpointFollower::new("speakers".into());
        assert_eq!(follower.observe("headset".into()), None);
        follower.reset();
        assert_eq!(follower.observe("headset".into()), None);
        assert_eq!(
            follower.observe("headset".into()),
            Some("headset".to_string())
        );
    }

    /// Polls `watch` every 200 ms (the session's tick) from `from` to `to`
    /// seconds, with the OS reporting `endpoint(t)`, and returns what the
    /// session saw at each tick plus how many reads were made.
    fn ticks(
        watch: &mut DeviceWatch,
        from_ms: u64,
        to_ms: u64,
        endpoint: impl Fn(u64) -> Option<&'static str>,
    ) -> (Vec<(u64, Option<String>)>, usize) {
        let mut seen = Vec::new();
        let mut reads = 0;
        for ms in (from_ms..to_ms).step_by(200) {
            let answer = watch.poll(ms * 1_000_000, || {
                reads += 1;
                endpoint(ms).map(str::to_string)
            });
            seen.push((ms, answer));
        }
        (seen, reads)
    }

    fn first_tick_reporting(seen: &[(u64, Option<String>)], id: &str) -> Option<u64> {
        seen.iter()
            .find(|(_, answer)| answer.as_deref() == Some(id))
            .map(|(ms, _)| *ms)
    }

    #[test]
    fn the_endpoint_is_read_only_every_check_interval() {
        let mut watch = DeviceWatch::new();
        let (seen, reads) = ticks(&mut watch, 0, 10_000, |_| Some("speakers"));
        assert_eq!(reads, 5, "one read at 0 s, then every 2 s");
        assert!(
            seen.iter()
                .all(|(_, answer)| answer.as_deref() == Some("speakers"))
        );
    }

    #[test]
    fn a_headset_plugged_in_is_reported_after_two_agreeing_reads() {
        let mut watch = DeviceWatch::new();
        // Plugged in at 3.1 s: read at 4 s (1st), 6 s (2nd, confirmed).
        let (seen, _) = ticks(&mut watch, 0, 12_000, |ms| {
            Some(if ms < 3_100 { "speakers" } else { "headset" })
        });
        assert_eq!(first_tick_reporting(&seen, "headset"), Some(6_000));
        // Nothing in between flips back and forth.
        let flips = seen.windows(2).filter(|w| w[0].1 != w[1].1).count();
        assert_eq!(flips, 1);
    }

    #[test]
    fn a_one_read_flap_is_not_a_switch() {
        let mut watch = DeviceWatch::new();
        // The headset's hands-free endpoint is default for one read only.
        let (seen, _) = ticks(&mut watch, 0, 12_000, |ms| {
            Some(if (3_100..5_000).contains(&ms) {
                "headset-hands-free"
            } else {
                "speakers"
            })
        });
        assert_eq!(first_tick_reporting(&seen, "headset-hands-free"), None);
    }

    #[test]
    fn a_failed_read_keeps_the_endpoint_and_cancels_a_switch_under_way() {
        let mut watch = DeviceWatch::new();
        // headset at 2 s, a failed read at 4 s, headset again at 6 s and 8 s.
        let (seen, _) = ticks(&mut watch, 0, 10_000, |ms| match ms {
            0..2_000 => Some("speakers"),
            4_000..6_000 => None,
            _ => Some("headset"),
        });
        assert!(
            seen.iter()
                .take_while(|(ms, _)| *ms < 8_000)
                .all(|(_, answer)| answer.as_deref() == Some("speakers"))
        );
        assert_eq!(first_tick_reporting(&seen, "headset"), Some(8_000));
    }

    #[test]
    fn no_default_endpoint_is_none_until_one_appears() {
        let mut watch = DeviceWatch::new();
        let (seen, _) = ticks(&mut watch, 0, 6_000, |ms| {
            (ms >= 3_000).then_some("speakers")
        });
        assert_eq!(seen[0].1, None);
        // Read at 2 s (none), then 4 s: taken at once, there was nothing to
        // confirm a switch away from.
        assert_eq!(first_tick_reporting(&seen, "speakers"), Some(4_000));
    }

    #[test]
    fn a_stale_watch_starts_over_instead_of_reporting_a_switch() {
        let mut watch = DeviceWatch::new();
        assert_eq!(
            watch.poll(0, || Some("speakers".into())).as_deref(),
            Some("speakers")
        );
        // An hour later, a new recording: the default is now the headset.
        let later = 3_600 * S;
        assert_eq!(
            watch.poll(later, || Some("headset".into())).as_deref(),
            Some("headset"),
            "the new recording's baseline, not a switch it must confirm"
        );
    }

    #[test]
    fn endpoint_keys_are_stable_and_distinct() {
        let speakers = "{0.0.0.00000000}.{1b2c3d4e-0000-0000-0000-000000000001}";
        let headset = "{0.0.0.00000000}.{1b2c3d4e-0000-0000-0000-000000000002}";
        assert_eq!(endpoint_key(speakers), endpoint_key(speakers));
        assert_ne!(endpoint_key(speakers), endpoint_key(headset));
        // FNV-1a's published value for the empty string.
        assert_eq!(endpoint_key(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(endpoint_key("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
