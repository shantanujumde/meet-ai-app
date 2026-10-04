//! Capture timestamps to a timeline, and the gaps in it (TUR-37).
//!
//! `cpal` gives a loopback packet no device position, only the instant its
//! first frame was captured (`InputCallbackInfo::timestamp().capture`, which
//! on WASAPI is `GetBuffer`'s QPC position on a 100 ns grid). The frames the
//! track holds and the time those frames cover only stay equal if every
//! stretch the stream skipped becomes silence. WASAPI skips in two ways:
//! loopback sends no packets at all while nothing plays (when the silence
//! keepalive is not running), and a late reader loses packets. Both show up
//! as one packet starting later than the previous one ended, which is all
//! [`Timeline::stamp`] looks at.

/// A packet that starts more than this much after the previous one ended,
/// and more than one previous packet's length, is a gap. Below it, timing
/// jitter. WASAPI's shared-mode period is 10 ms.
pub const GAP_THRESHOLD_MIN_NS: u64 = 10_000_000;

/// The most silence one gap is filled with: 10 minutes. A longer gap is not
/// a quiet stretch but something else (a sleep, a clock jump); it is filled
/// up to this much and logged.
pub const MAX_GAP_FILL_NS: u64 = 600 * 1_000_000_000;

/// `QueryPerformanceCounter` ticks at `frequency` Hz, to nanoseconds on the
/// same 100 ns grid `cpal` 0.18.2 puts WASAPI's capture times on
/// (`wasapi/stream.rs`, `Stream::now`), so a host-clock read and a capture
/// time can be compared exactly. 0 for a zero frequency.
// Adapted from github.com/RustAudio/cpal/src/host/wasapi/stream.rs @ e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7 (Apache-2.0)
pub fn qpc_to_ns(counter: u64, frequency: u64) -> u64 {
    if frequency == 0 {
        return 0;
    }
    let units_100ns = u128::from(counter) * 10_000_000 / u128::from(frequency);
    u64::try_from(units_100ns * 100).unwrap_or(u64::MAX)
}

/// A callback time within this much of the host clock's "now" is taken to
/// be on the host clock already (TUR-38). `cpal`'s PipeWire host stamps
/// callbacks with `pw_stream_get_time_n`, which is CLOCK_MONOTONIC, the
/// Linux host clock. Its PulseAudio and ALSA hosts count from when the
/// stream was built instead, which is nowhere near the uptime-sized host
/// clock.
pub const SAME_CLOCK_WINDOW_NS: u64 = 1_000_000_000;

/// A callback's capture time on the host clock, from `cpal`'s two
/// timestamps (`callback`, `capture`, in ns on the stream's own clock) and
/// the host clock read in that callback (`now_ns`), TUR-38.
///
/// * The callback time is within [`SAME_CLOCK_WINDOW_NS`] of `now_ns`: the
///   stream runs on the host clock, and `capture` is used as is.
/// * Otherwise the stream has a clock of its own, and only the latency
///   between the two times is kept: `now_ns - (callback - capture)`. The
///   error is however long the callback took to get here, microseconds.
///
/// `None` when the stream gave no time at all, so the caller stamps the
/// callback with `now_ns`.
pub fn capture_on_host_clock(callback_ns: u64, capture_ns: u64, now_ns: u64) -> Option<u64> {
    if callback_ns == 0 && capture_ns == 0 {
        return None;
    }
    if now_ns.abs_diff(callback_ns) <= SAME_CLOCK_WINDOW_NS {
        return (capture_ns != 0).then_some(capture_ns);
    }
    let latency = callback_ns.saturating_sub(capture_ns);
    Some(now_ns.saturating_sub(latency)).filter(|&ns| ns != 0)
}

/// How long `frames` last at `rate`, in ns. 0 for a zero rate.
pub fn frames_to_ns(frames: u64, rate: u32) -> u64 {
    if rate == 0 {
        return 0;
    }
    u64::try_from(u128::from(frames) * 1_000_000_000 / u128::from(rate)).unwrap_or(u64::MAX)
}

/// How many frames at `rate` last `ns`, rounded to the nearest.
pub fn ns_to_frames(ns: u64, rate: u32) -> u64 {
    let frames = (u128::from(ns) * u128::from(rate) + 500_000_000) / 1_000_000_000;
    u64::try_from(frames).unwrap_or(u64::MAX)
}

/// What one packet's timestamp says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// When the packet's first frame was captured, in the host-clock domain.
    pub host_ns: u64,
    /// Frames of silence that belong before this packet: the stretch between
    /// the previous packet's end and this one's start, when that is a gap.
    pub gap_frames: u64,
    /// The gap was longer than [`MAX_GAP_FILL_NS`] and only partly filled.
    pub gap_capped: bool,
}

/// The packets seen so far, as far as their timing goes. No allocation and
/// integer maths only, so it runs inside the capture callback.
#[derive(Debug, Default, Clone)]
pub struct Timeline {
    /// The previous packet's start and its frame count.
    last: Option<(u64, u64)>,
    /// The smallest jump that counts as a gap; `None` is
    /// [`GAP_THRESHOLD_MIN_NS`] (TUR-38).
    min_gap_ns: Option<u64>,
}

impl Timeline {
    /// A timeline whose gaps must also be longer than `min_gap_ns`, for a
    /// stream whose callbacks can run late without losing audio (TUR-38).
    pub fn with_min_gap(min_gap_ns: u64) -> Self {
        Self {
            last: None,
            min_gap_ns: Some(min_gap_ns),
        }
    }

    /// One packet: its capture time (`None` or 0 when the OS gave none),
    /// how many frames it holds, and the rate they run at.
    ///
    /// * A packet with no usable time (none, or not after the previous one)
    ///   is placed right after the previous packet, so the timeline never
    ///   goes backwards. Before any packet had a time, `None`.
    /// * A packet starting more than one previous packet's length (and at
    ///   least [`GAP_THRESHOLD_MIN_NS`]) after the previous one ended gets
    ///   that whole stretch as [`Stamp::gap_frames`].
    /// * Anything closer is jitter: its own time, no gap.
    pub fn stamp(&mut self, capture_ns: Option<u64>, frames: u64, rate: u32) -> Option<Stamp> {
        let raw = capture_ns.filter(|&ns| ns != 0);
        let Some((last_ns, last_frames)) = self.last else {
            let host_ns = raw?;
            self.last = Some((host_ns, frames));
            return Some(Stamp {
                host_ns,
                gap_frames: 0,
                gap_capped: false,
            });
        };
        let last_len = frames_to_ns(last_frames, rate);
        let expected = last_ns.saturating_add(last_len);
        let mut stamp = Stamp {
            host_ns: expected,
            gap_frames: 0,
            gap_capped: false,
        };
        if let Some(ns) = raw.filter(|&ns| ns > last_ns) {
            stamp.host_ns = ns;
            let late = ns.saturating_sub(expected);
            let floor = self.min_gap_ns.unwrap_or(GAP_THRESHOLD_MIN_NS);
            if late > last_len.max(floor) {
                stamp.gap_capped = late > MAX_GAP_FILL_NS;
                stamp.gap_frames = ns_to_frames(late.min(MAX_GAP_FILL_NS), rate);
            }
        }
        self.last = Some((stamp.host_ns, frames));
        Some(stamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;
    const RATE: u32 = 48_000;
    /// One 10 ms WASAPI period at 48 kHz.
    const PERIOD: u64 = 480;

    fn stamp(t: &mut Timeline, ns: u64) -> Stamp {
        t.stamp(Some(ns), PERIOD, RATE).expect("timed packet")
    }

    #[test]
    fn qpc_ticks_land_on_cpals_100_ns_grid() {
        // A 10 MHz QPC (the usual one) is already in 100 ns units.
        assert_eq!(qpc_to_ns(123_456_789, 10_000_000), 12_345_678_900);
        // A 3 MHz one: 3 ticks = 1 µs.
        assert_eq!(qpc_to_ns(3_000_000, 3_000_000), 1_000_000_000);
        assert_eq!(qpc_to_ns(7, 3_000_000), 2_300);
        assert_eq!(qpc_to_ns(42, 0), 0);
        // A day of uptime at 10 MHz does not overflow.
        assert_eq!(
            qpc_to_ns(86_400 * 10_000_000, 10_000_000),
            86_400 * 1_000_000_000
        );
    }

    #[test]
    fn a_stream_on_the_host_clock_keeps_its_capture_time() {
        // PipeWire: both times are CLOCK_MONOTONIC, an hour of uptime in.
        let now = 3_600_000 * MS;
        let callback = now - 2 * MS;
        let capture = callback - 21 * MS;
        assert_eq!(capture_on_host_clock(callback, capture, now), Some(capture));
        // A little ahead of our read is the same clock too.
        assert_eq!(
            capture_on_host_clock(now + 5 * MS, now - 16 * MS, now),
            Some(now - 16 * MS)
        );
    }

    #[test]
    fn a_stream_on_its_own_clock_keeps_only_the_latency() {
        // PulseAudio: 1.5 s since the stream was built, 40 ms of latency.
        let now = 3_600_000 * MS;
        let mapped = capture_on_host_clock(1_500 * MS, 1_460 * MS, now);
        assert_eq!(mapped, Some(now - 40 * MS));
        // A capture time past the callback time is no latency, not negative.
        assert_eq!(
            capture_on_host_clock(1_500 * MS, 1_600 * MS, now),
            Some(now)
        );
    }

    #[test]
    fn no_stream_time_leaves_it_to_the_caller() {
        let now = 3_600_000 * MS;
        assert_eq!(capture_on_host_clock(0, 0, now), None);
        // On the host clock but with no capture time.
        assert_eq!(capture_on_host_clock(now, 0, now), None);
        // A host clock read of 0 maps to nothing usable either.
        assert_eq!(capture_on_host_clock(5_000 * MS, 4_000 * MS, 0), None);
    }

    #[test]
    fn frame_and_ns_conversions_round_trip() {
        assert_eq!(frames_to_ns(480, 48_000), 10 * MS);
        assert_eq!(frames_to_ns(441, 44_100), 10 * MS);
        assert_eq!(frames_to_ns(1, 0), 0);
        assert_eq!(ns_to_frames(10 * MS, 48_000), 480);
        assert_eq!(ns_to_frames(500 * MS, 16_000), 8_000);
        // Rounds to the nearest frame, not down.
        assert_eq!(ns_to_frames(20_900, 48_000), 1);
        assert_eq!(ns_to_frames(1, 0), 0);
    }

    #[test]
    fn the_first_packet_needs_a_time() {
        let mut t = Timeline::default();
        assert_eq!(t.stamp(None, PERIOD, RATE), None);
        assert_eq!(t.stamp(Some(0), PERIOD, RATE), None);
        assert_eq!(
            stamp(&mut t, 5_000 * MS),
            Stamp {
                host_ns: 5_000 * MS,
                gap_frames: 0,
                gap_capped: false
            }
        );
    }

    #[test]
    fn back_to_back_packets_have_no_gap() {
        let mut t = Timeline::default();
        for n in 0..100 {
            let s = stamp(&mut t, 1_000 * MS + n * 10 * MS);
            assert_eq!(s.gap_frames, 0, "packet {n}");
            assert_eq!(s.host_ns, 1_000 * MS + n * 10 * MS);
        }
    }

    #[test]
    fn jitter_under_one_buffer_is_not_a_gap() {
        let mut t = Timeline::default();
        stamp(&mut t, 1_000 * MS);
        // 9 ms late: under one 10 ms buffer.
        let s = stamp(&mut t, 1_019 * MS);
        assert_eq!(s.gap_frames, 0);
        assert_eq!(s.host_ns, 1_019 * MS);
        // Early is never a gap either.
        let s = stamp(&mut t, 1_025 * MS);
        assert_eq!(s.gap_frames, 0);
    }

    #[test]
    fn a_jump_of_more_than_one_buffer_becomes_silence() {
        let mut t = Timeline::default();
        stamp(&mut t, 1_000 * MS);
        // Nothing played for 2 s: the next packet starts 2 s after the
        // previous one ended.
        let s = stamp(&mut t, 1_010 * MS + 2_000 * MS);
        assert_eq!(s.gap_frames, 2 * u64::from(RATE));
        assert!(!s.gap_capped);
        assert_eq!(s.host_ns, 3_010 * MS);
        // And the timeline carries on from there.
        assert_eq!(stamp(&mut t, 3_020 * MS).gap_frames, 0);
    }

    #[test]
    fn small_packets_still_need_the_minimum_threshold() {
        // 1 ms packets: a 5 ms late one is over "one buffer" but under the
        // 10 ms floor, so it is jitter.
        let mut t = Timeline::default();
        t.stamp(Some(1_000 * MS), 48, RATE);
        let s = t.stamp(Some(1_006 * MS), 48, RATE).expect("timed");
        assert_eq!(s.gap_frames, 0);
        let s = t.stamp(Some(1_030 * MS), 48, RATE).expect("timed");
        assert_eq!(s.gap_frames, ns_to_frames(23 * MS, RATE));
    }

    #[test]
    fn a_packet_with_no_time_or_a_backwards_one_follows_the_previous() {
        let mut t = Timeline::default();
        stamp(&mut t, 1_000 * MS);
        let s = t.stamp(None, PERIOD, RATE).expect("extrapolated");
        assert_eq!(s.host_ns, 1_010 * MS);
        assert_eq!(s.gap_frames, 0);
        // Backwards: still forwards, from the extrapolated one.
        let s = stamp(&mut t, 900 * MS);
        assert_eq!(s.host_ns, 1_020 * MS);
        // The same time twice is not progress either.
        let s = stamp(&mut t, 1_020 * MS);
        assert_eq!(s.host_ns, 1_030 * MS);
        // A real time again is taken as is.
        assert_eq!(stamp(&mut t, 1_040 * MS).host_ns, 1_040 * MS);
    }

    #[test]
    fn a_raised_gap_floor_lets_late_callbacks_through() {
        // A callback 60 ms late, then the queued one right behind it: under
        // a 100 ms floor that is jitter, not 60 ms of invented silence.
        let mut t = Timeline::with_min_gap(100 * MS);
        stamp(&mut t, 1_000 * MS);
        assert_eq!(stamp(&mut t, 1_070 * MS).gap_frames, 0);
        assert_eq!(stamp(&mut t, 1_071 * MS).gap_frames, 0);
        // A real silence of 2 s is still filled.
        let s = stamp(&mut t, 1_081 * MS + 2_000 * MS);
        assert_eq!(s.gap_frames, 2 * u64::from(RATE));
    }

    #[test]
    fn a_huge_gap_is_capped() {
        let mut t = Timeline::default();
        stamp(&mut t, 1_000 * MS);
        let s = stamp(&mut t, 1_010 * MS + 2 * MAX_GAP_FILL_NS);
        assert!(s.gap_capped);
        assert_eq!(s.gap_frames, ns_to_frames(MAX_GAP_FILL_NS, RATE));
    }

    #[test]
    fn frames_plus_gaps_track_wall_time() {
        // 1 s of packets, a 3 s hole, 1 s of packets: 5 s of frames.
        let mut t = Timeline::default();
        let mut frames = 0;
        let mut ns = 7_000 * MS;
        for n in 0..200 {
            if n == 100 {
                ns += 3_000 * MS;
            }
            let s = stamp(&mut t, ns);
            frames += s.gap_frames + PERIOD;
            ns += 10 * MS;
        }
        assert_eq!(frames, 5 * u64::from(RATE));
    }
}
