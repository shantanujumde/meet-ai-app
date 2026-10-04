//! The capture callback's side of a loopback source (TUR-37): one OS packet
//! in, samples and gap marks out, without allocating, locking or blocking.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ringbuf::HeapProd;
use ringbuf::traits::{Observer, Producer};

use super::clock::{Timeline, frames_to_ns};
use super::silent;
use super::splice::GapMark;
use crate::rate_meter::{CallbackMeter, FixedRates, Rates};

/// What the callback did that the worker should log. Atomics only, so the
/// callback can count without a lock.
#[derive(Debug, Default)]
pub(crate) struct CaptureStats {
    /// Timestamp gaps filled with silence, and their frames in total.
    pub gaps: AtomicU64,
    pub gap_frames: AtomicU64,
    /// Gaps longer than `clock::MAX_GAP_FILL_NS`, only partly filled.
    pub capped_gaps: AtomicU64,
    /// Frames the sample ring had no room for, replaced with silence.
    pub dropped_frames: AtomicU64,
    /// Gap marks the mark ring had no room for: that silence is missing.
    pub lost_marks: AtomicU64,
    /// Packets flagged silent and zero-filled.
    pub silent_packets: AtomicU64,
    /// Non-finite samples replaced with 0.
    pub non_finite: AtomicU64,
}

impl CaptureStats {
    pub(crate) fn describe(&self) -> String {
        let get = |n: &AtomicU64| n.load(Ordering::Relaxed);
        format!(
            "{} timestamp gaps filled ({} frames, {} capped), {} frames dropped by a full ring, \
             {} gap marks lost, {} silent packets, {} non-finite samples",
            get(&self.gaps),
            get(&self.gap_frames),
            get(&self.capped_gaps),
            get(&self.dropped_frames),
            get(&self.lost_marks),
            get(&self.silent_packets),
            get(&self.non_finite),
        )
    }
}

/// Handed to [`super::source::Backend::start_capture`]; the backend calls
/// [`Capture::packet`] from its capture callback for every OS packet.
pub struct Capture {
    producer: HeapProd<f32>,
    marks: HeapProd<GapMark>,
    timeline: Timeline,
    meter: CallbackMeter<FixedRates>,
    rates: Arc<FixedRates>,
    /// The latest packet's capture time, for the worker's position latch.
    last_ns: Arc<AtomicU64>,
    channels: usize,
    /// Interleaved samples pushed so far: where the next gap mark points.
    pushed: u64,
    /// The silence filled in so far, in ns: taken off the times the rate
    /// meter sees, so a gap does not read as a slow device.
    filled_ns: u64,
    stats: Arc<CaptureStats>,
}

impl Capture {
    pub(crate) fn new(
        producer: HeapProd<f32>,
        marks: HeapProd<GapMark>,
        rates: Arc<FixedRates>,
        last_ns: Arc<AtomicU64>,
        channels: usize,
        stats: Arc<CaptureStats>,
    ) -> Self {
        let channels = channels.max(1);
        Self {
            producer,
            marks,
            timeline: Timeline::default(),
            meter: CallbackMeter::new(Arc::clone(&rates), channels),
            rates,
            last_ns,
            channels,
            pushed: 0,
            filled_ns: 0,
            stats,
        }
    }

    /// Gaps must also be longer than `min_gap_ns` ([`Timeline::with_min_gap`],
    /// TUR-38). Before the first packet only.
    pub(crate) fn with_min_gap(mut self, min_gap_ns: u64) -> Self {
        self.timeline = Timeline::with_min_gap(min_gap_ns);
        self
    }

    /// One packet from the OS: interleaved `samples` (conditioned in place),
    /// the instant its first frame was captured (`None` when the OS gave
    /// none), and whether the OS flagged it silent.
    ///
    /// Silence owed before the packet (a timestamp gap) and in place of
    /// samples the ring had no room for goes to the worker as a gap mark, so
    /// the track keeps wall time either way. Only whole frames are pushed,
    /// so a full ring can never shift the channels.
    pub fn packet(&mut self, samples: &mut [f32], capture_ns: Option<u64>, silent: bool) {
        let done = silent::condition(samples, silent);
        if done.zero_filled {
            self.stats.silent_packets.fetch_add(1, Ordering::Relaxed);
        }
        if done.non_finite > 0 {
            self.stats
                .non_finite
                .fetch_add(done.non_finite as u64, Ordering::Relaxed);
        }

        let channels = self.channels;
        let whole = samples.len() / channels * channels;
        let frames = (whole / channels) as u64;
        let rate = self.rates.effective();
        let stamp = self.timeline.stamp(capture_ns, frames, rate);

        if let Some(stamp) = stamp
            && stamp.gap_frames > 0
        {
            self.mark(stamp.gap_frames);
            self.filled_ns = self
                .filled_ns
                .saturating_add(frames_to_ns(stamp.gap_frames, rate));
            self.stats.gaps.fetch_add(1, Ordering::Relaxed);
            self.stats
                .gap_frames
                .fetch_add(stamp.gap_frames, Ordering::Relaxed);
            if stamp.gap_capped {
                self.stats.capped_gaps.fetch_add(1, Ordering::Relaxed);
            }
        }

        let room = self.producer.vacant_len() / channels * channels;
        let pushed = self.producer.push_slice(&samples[..whole.min(room)]);
        self.pushed += pushed as u64;
        let dropped = ((whole - pushed) / channels) as u64;
        if dropped > 0 {
            self.mark(dropped);
            self.stats
                .dropped_frames
                .fetch_add(dropped, Ordering::Relaxed);
        }

        if let Some(stamp) = stamp {
            // Dropped frames were delivered on time, so the meter counts them.
            self.meter
                .observe(stamp.host_ns.saturating_sub(self.filled_ns), whole);
            self.last_ns.store(stamp.host_ns, Ordering::Release);
        }
    }

    fn mark(&mut self, frames: u64) {
        let mark = GapMark {
            at_sample: self.pushed,
            frames,
        };
        if self.marks.try_push(mark).is_err() {
            self.stats.lost_marks.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use ringbuf::HeapRb;
    use ringbuf::traits::{Consumer, Split};

    use super::*;

    const MS: u64 = 1_000_000;

    struct Rig {
        capture: Capture,
        samples: ringbuf::HeapCons<f32>,
        marks: ringbuf::HeapCons<GapMark>,
        last_ns: Arc<AtomicU64>,
        stats: Arc<CaptureStats>,
    }

    fn rig(rate: u32, channels: usize, ring: usize, mark_ring: usize) -> Rig {
        let (producer, samples) = HeapRb::<f32>::new(ring).split();
        let (mark_tx, marks) = HeapRb::<GapMark>::new(mark_ring).split();
        let last_ns = Arc::new(AtomicU64::new(0));
        let stats = Arc::new(CaptureStats::default());
        let capture = Capture::new(
            producer,
            mark_tx,
            FixedRates::new("test", rate),
            Arc::clone(&last_ns),
            channels,
            Arc::clone(&stats),
        );
        Rig {
            capture,
            samples,
            marks,
            last_ns,
            stats,
        }
    }

    fn drain(rig: &mut Rig) -> (Vec<f32>, Vec<GapMark>) {
        (
            rig.samples.pop_iter().collect(),
            rig.marks.pop_iter().collect(),
        )
    }

    #[test]
    fn a_timestamp_gap_marks_silence_before_the_packet() {
        let mut rig = rig(1_000, 2, 1_000, 8);
        // 10 frames = 10 ms at 1 kHz.
        rig.capture.packet(&mut [0.5; 20], Some(1_000 * MS), false);
        rig.capture.packet(&mut [0.5; 20], Some(1_510 * MS), false);
        let (samples, marks) = drain(&mut rig);
        assert_eq!(samples.len(), 40);
        assert_eq!(
            marks,
            vec![GapMark {
                at_sample: 20,
                frames: 500
            }]
        );
        assert_eq!(rig.last_ns.load(Ordering::Acquire), 1_510 * MS);
        assert_eq!(rig.stats.gaps.load(Ordering::Relaxed), 1);
        assert_eq!(rig.stats.gap_frames.load(Ordering::Relaxed), 500);
    }

    #[test]
    fn a_full_ring_turns_the_lost_frames_into_silence_on_a_frame_boundary() {
        // Room for 5 stereo samples: only 2 whole frames fit.
        let mut rig = rig(48_000, 2, 5, 8);
        rig.capture
            .packet(&mut [0.1, 0.2, 0.3, 0.4, 0.5, 0.6], Some(1_000 * MS), false);
        let (samples, marks) = drain(&mut rig);
        assert_eq!(samples, vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(
            marks,
            vec![GapMark {
                at_sample: 4,
                frames: 1
            }]
        );
        assert_eq!(rig.stats.dropped_frames.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_silent_packet_reaches_the_ring_as_zeros() {
        let mut rig = rig(48_000, 1, 100, 8);
        rig.capture.packet(&mut [0.9; 4], Some(1_000 * MS), true);
        rig.capture
            .packet(&mut [f32::NAN, 0.25], Some(1_001 * MS), false);
        let (samples, _) = drain(&mut rig);
        assert_eq!(samples, vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.25]);
        assert_eq!(rig.stats.silent_packets.load(Ordering::Relaxed), 1);
        assert_eq!(rig.stats.non_finite.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_trailing_partial_frame_is_not_pushed() {
        let mut rig = rig(48_000, 2, 100, 8);
        rig.capture
            .packet(&mut [0.1, 0.2, 0.3], Some(1_000 * MS), false);
        let (samples, marks) = drain(&mut rig);
        assert_eq!(samples, vec![0.1, 0.2]);
        assert!(marks.is_empty());
    }

    #[test]
    fn packets_before_the_first_timestamp_still_go_through() {
        let mut rig = rig(48_000, 1, 100, 8);
        rig.capture.packet(&mut [0.1, 0.2], None, false);
        assert_eq!(rig.last_ns.load(Ordering::Acquire), 0, "no position yet");
        rig.capture.packet(&mut [0.3], Some(5 * MS), false);
        let (samples, _) = drain(&mut rig);
        assert_eq!(samples, vec![0.1, 0.2, 0.3]);
        assert_eq!(rig.last_ns.load(Ordering::Acquire), 5 * MS);
    }

    #[test]
    fn a_full_mark_ring_is_counted() {
        let mut rig = rig(1_000, 1, 1_000, 1);
        let mut ns = 1_000 * MS;
        for _ in 0..3 {
            rig.capture.packet(&mut [0.5; 10], Some(ns), false);
            ns += 500 * MS;
        }
        assert_eq!(rig.stats.lost_marks.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn gaps_do_not_read_as_a_slow_device() {
        // 3 s of 10 ms packets at 48 kHz with a 1 s hole every 200 ms: the
        // meter must still see 48 kHz, not a fraction of it.
        let mut rig = rig(48_000, 1, 1 << 20, 64);
        let mut ns = 1_000 * MS;
        let mut packet = vec![0.0f32; 480];
        for n in 0..300 {
            if n % 20 == 19 {
                ns += 1_000 * MS;
            }
            rig.capture.packet(&mut packet, Some(ns), false);
            ns += 10 * MS;
            let _ = drain(&mut rig);
        }
        assert_eq!(rig.capture.rates.effective(), 48_000);
        let measured = rig.capture.rates.cell().get().expect("a measurement");
        assert!((measured - 48_000.0).abs() < 500.0, "measured {measured}");
    }
}
