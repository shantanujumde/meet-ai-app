use std::path::Path;

use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};

use super::*;
use crate::rate_meter::FixedRates;

const MS: u64 = 1_000_000;

#[test]
fn no_mark_means_no_time() {
    let (_tx, mut clock) = time_marks(2);
    clock.consumed(100);
    assert_eq!(clock.boundary_ns(48_000, 0.0), 0);
}

#[test]
fn the_boundary_counts_on_from_the_latest_mark_and_back_by_what_is_buffered() {
    let (mut tx, mut clock) = time_marks(2);
    tx.mark(1_000 * MS);
    tx.advance(960); // 480 stereo frames = 10 ms at 48 kHz
    tx.mark(1_010 * MS);
    tx.advance(960);
    clock.consumed(1_440); // 720 frames: 5 ms into the second packet
    assert_eq!(clock.boundary_ns(48_000, 0.0), 1_015 * MS);
    // 240 frames still inside the pipeline: their output is not out yet.
    assert_eq!(clock.boundary_ns(48_000, 240.0), 1_010 * MS);
}

#[test]
fn a_later_mark_at_the_same_sample_wins() {
    // A full ring took none of a packet's samples: the next packet's mark
    // lands on the same sample, and is the one that is true.
    let (mut tx, mut clock) = time_marks(1);
    tx.mark(1_000 * MS);
    tx.advance(0);
    tx.mark(1_200 * MS);
    tx.advance(16);
    clock.consumed(16);
    assert_eq!(clock.boundary_ns(16_000, 0.0), 1_201 * MS);
}

#[test]
fn samples_before_the_first_timed_packet_count_back_from_it() {
    let (mut tx, mut clock) = time_marks(1);
    tx.advance(160); // untimed packet
    tx.mark(1_000 * MS);
    tx.advance(160);
    clock.consumed(160);
    assert_eq!(clock.boundary_ns(16_000, 0.0), 1_000 * MS);
    clock.consumed(80);
    assert_eq!(clock.boundary_ns(16_000, 0.0), 1_005 * MS);
}

#[test]
fn a_lost_mark_is_carried_on_from_the_one_before() {
    let (mut tx, mut clock) = time_marks(1);
    for n in 0..(MARK_CAPACITY as u64 + 10) {
        tx.mark(1_000 * MS + n * 10 * MS);
        tx.advance(160);
    }
    // The ring kept the first MARK_CAPACITY marks; the rest are extrapolated.
    clock.consumed((MARK_CAPACITY + 10) * 160);
    let expected = 1_000 * MS + (MARK_CAPACITY as u64 + 10) * 10 * MS;
    assert_eq!(clock.boundary_ns(16_000, 0.0), expected);
}

#[test]
fn no_capture_time_falls_back_to_the_callback_less_its_packet() {
    assert_eq!(
        first_frame_ns(Some(95 * MS), 100 * MS, 480, 48_000),
        95 * MS
    );
    assert_eq!(first_frame_ns(None, 100 * MS, 480, 48_000), 90 * MS);
    // A time on some other clock is not believed either.
    assert_eq!(
        first_frame_ns(Some(5 * MS), 9_000 * MS, 480, 48_000),
        8_990 * MS
    );
}

#[test]
fn only_a_recent_capture_time_is_believed() {
    let now = 10_000 * MS;
    assert_eq!(
        plausible_capture_ns(now - 20 * MS, now),
        Some(now - 20 * MS)
    );
    assert_eq!(plausible_capture_ns(now, now), Some(now));
    assert_eq!(plausible_capture_ns(0, now), None);
    assert_eq!(plausible_capture_ns(now - 2_000 * MS, now), None, "too old");
    assert_eq!(plausible_capture_ns(now + 50 * MS, now), None, "future");
}

/// A capture rig: a callback that pushes packets with their capture times,
/// and a worker [`Feed`] into a real [`TrackWriter`].
struct Rig {
    producer: ringbuf::HeapProd<f32>,
    consumer: ringbuf::HeapCons<f32>,
    marks: MarkWriter,
    feed: Feed,
    pipeline: Pipeline,
    rates: std::sync::Arc<FixedRates>,
    track: TrackWriter,
    rate: u32,
    channels: usize,
    /// Device frames delivered so far.
    delivered: u64,
}

const T0: u64 = 5_000 * MS;

impl Rig {
    fn new(path: &Path, rate: u32, channels: usize) -> Self {
        let (producer, consumer) = HeapRb::<f32>::new(rate as usize * channels * 4).split();
        let (marks, clock) = time_marks(channels);
        let track = TrackWriter::open(path, "test").unwrap();
        Self {
            producer,
            consumer,
            marks,
            feed: Feed::new(track.clone(), clock, None),
            pipeline: Pipeline::new("test", channels, rate),
            rates: FixedRates::new("test", rate),
            track,
            rate,
            channels,
            delivered: 0,
        }
    }

    /// One callback: `frames` of a tone, captured from frame `delivered` on.
    fn callback(&mut self, frames: usize) {
        let ns = T0 + self.delivered * 1_000_000_000 / u64::from(self.rate);
        let tone = test_support::sine_f32(frames, self.rate, 440.0, 0.5);
        let samples: Vec<f32> = tone
            .iter()
            .flat_map(|&s| std::iter::repeat_n(s, self.channels))
            .collect();
        self.marks.mark(ns);
        self.marks.advance(self.producer.push_slice(&samples));
        self.delivered += frames as u64;
    }

    /// The worker: take at most `max` samples and feed them on.
    fn work(&mut self, max: usize) {
        let mut scratch = vec![0.0f32; max];
        let popped = self.consumer.pop_slice(&mut scratch);
        self.feed
            .push(&mut self.pipeline, &*self.rates, &scratch[..popped]);
    }

    /// When frame 0 was captured, by this channel's own position.
    fn frame_zero_ns(&self) -> Option<f64> {
        let (ns, frames) = self.track.position()?;
        Some(ns as f64 - frames as f64 * 1e9 / f64::from(SAMPLE_RATE_HZ))
    }
}

/// TUR-151's anchor half: whatever the ring's backlog and the resampler's
/// pending input, every position names the instant its frame was captured,
/// so frame 0 always reads back as the first packet's capture time.
#[test]
fn every_position_names_the_capture_time_of_its_frame() {
    for (rate, channels, packet) in [(48_000, 2, 512), (16_000, 1, 160), (44_100, 2, 441)] {
        let dir = tempfile::tempdir().unwrap();
        let mut rig = Rig::new(&dir.path().join("t.wav"), rate, channels);
        let mut checked = 0;
        for n in 0..400 {
            rig.callback(packet);
            // A worker that falls behind for a while, then catches up in
            // uneven slices: the backlog and the pending chunk keep moving.
            if n % 7 != 0 {
                rig.work(1_000 + 37 * (n % 5));
            }
            if let Some(zero) = rig.frame_zero_ns() {
                let error_ms = (zero - T0 as f64).abs() / 1e6;
                assert!(
                    error_ms < 0.2,
                    "{rate} Hz after {n} packets: frame 0 read {error_ms:.3} ms off"
                );
                checked += 1;
            }
        }
        assert!(checked > 300, "{rate} Hz: only {checked} positions");
    }
}

#[test]
fn gap_silence_written_straight_keeps_every_position_true() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(&dir.path().join("t.wav"), 16_000, 1);
    for _ in 0..20 {
        rig.callback(160);
        rig.work(4_096);
    }
    let before = rig.frame_zero_ns().unwrap();
    // 1 s of the device's silence owed before the next packet: the clock
    // counts it as consumed time, the way the loopback's next packet does.
    rig.delivered += 16_000;
    rig.callback(160);
    let mut scratch = [0.0f32; 160];
    let popped = rig.consumer.pop_slice(&mut scratch);
    assert_eq!(popped, 160);
    rig.feed.silence(16_000, &rig.pipeline);
    let after_gap = rig.frame_zero_ns().unwrap();
    rig.feed
        .push(&mut rig.pipeline, &*rig.rates, &scratch[..popped]);
    let after = rig.frame_zero_ns().unwrap();
    for (what, zero) in [("gap", after_gap), ("audio", after)] {
        assert!(
            (zero - before).abs() / 1e6 < 0.2,
            "after the {what}: frame 0 moved {:.3} ms",
            (zero - before) / 1e6
        );
    }
}
