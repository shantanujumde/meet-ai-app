//! Both halves of a capture ring, for every source (TUR-37; shared since
//! TUR-163): the [`Capture`] the OS callback pushes into, and the [`Drain`]
//! the worker runs, which takes samples through the [`Pipeline`] into the
//! track and writes the callback's [`GapMark`]s as silence.
//!
//! Until TUR-163 only the loopback worked this way. The microphone and the
//! macOS tap pushed what fit into their rings and threw the rest away: no
//! count, no log, no silence put back, so the track came out shorter than
//! the other channel, and a partial frame could swap a stereo pair. Now
//! every source gets its ring from [`ring`]: whole frames only, frames the
//! ring had no room for become counted silence with a warning, and the ring
//! holds [`RING_SECONDS`] of the device's own rate and channels, not of
//! 48 kHz stereo.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ringbuf::traits::{Consumer, Split};
use ringbuf::{HeapCons, HeapRb};

use super::capture::{Capture, CaptureStats};
use super::clock::{frames_to_ns, ns_to_frames};
use super::splice::{GapMark, Piece, splice};
use crate::capture_clock::{Feed, time_marks};
use crate::pipeline::Pipeline;
use crate::rate_meter::Rates;
use crate::segments::SAMPLE_RATE_HZ;
use crate::tee::Tee;
use crate::track::{IDLE_POLL, TrackWriter};

/// Seconds of device audio the sample ring holds for a late worker.
pub(crate) const RING_SECONDS: usize = 4;

/// The lowest rate a ring is sized for: a device that starts at 16 kHz (a
/// Bluetooth headset in a call) and moves to 48 kHz mid-recording keeps its
/// [`RING_SECONDS`].
const MIN_RING_RATE: u32 = 48_000;

/// Gap marks the callback can queue before the worker takes them.
const MARK_CAPACITY: usize = 256;

/// Samples the worker takes from the ring at a time, and the length of its
/// buffers of zeros for gap silence.
const WORKER_POP_SAMPLES: usize = 4096;

/// The start of every gap goes through the resampler as zeros, so the
/// audio before it (and the filter's tail) comes out first and in order;
/// the rest is written as 16 kHz silence directly, so a gap of minutes costs
/// no resampling. Two resampler chunks at least.
const FLUSH_THROUGH_MS: u64 = 100;
const FLUSH_THROUGH_MIN_FRAMES: u64 = 2048;

/// The sample ring's size for a device at `rate` Hz with `channels`
/// channels: [`RING_SECONDS`] of it, at [`MIN_RING_RATE`] or more.
pub(crate) fn ring_samples(rate: u32, channels: usize) -> usize {
    RING_SECONDS * rate.max(MIN_RING_RATE) as usize * channels.max(1)
}

/// The callback's and the worker's halves of one channel's ring, sized by
/// [`ring_samples`] for `rates`' rate right now and `channels`.
pub(crate) fn ring<R: Rates + 'static>(
    label: &'static str,
    rates: Arc<R>,
    channels: usize,
    track: TrackWriter,
    tee: Option<Tee>,
) -> (Capture, Drain<R>) {
    let samples = ring_samples(rates.effective(), channels);
    ring_sized(label, rates, channels, (track, tee), samples)
}

/// [`ring`] with a ring of `samples`; small ones in the tests.
pub(crate) fn ring_sized<R: Rates + 'static>(
    label: &'static str,
    rates: Arc<R>,
    channels: usize,
    (track, tee): (TrackWriter, Option<Tee>),
    samples: usize,
) -> (Capture, Drain<R>) {
    let channels = channels.max(1);
    let (producer, consumer) = HeapRb::<f32>::new(samples.max(channels)).split();
    let (mark_tx, marks) = HeapRb::<GapMark>::new(MARK_CAPACITY).split();
    // Each packet's capture time rides next to its samples (TUR-151).
    let (times, clock) = time_marks(channels);
    let stats = Arc::new(CaptureStats::default());
    let shared: Arc<dyn Rates> = Arc::clone(&rates) as Arc<dyn Rates>;
    let capture = Capture::new(
        producer,
        mark_tx,
        shared,
        times,
        channels,
        Arc::clone(&stats),
    );
    let drain = Drain {
        label,
        consumer,
        marks,
        pipeline: Pipeline::new(label, channels, rates.effective()),
        rates,
        feed: Feed::new(track, clock, tee),
        stats,
        channels,
    };
    (capture, drain)
}

/// The worker's half of a ring: ring to [`Pipeline`] to [`TrackWriter`],
/// with the gap marks spliced in as silence.
pub(crate) struct Drain<R: Rates> {
    label: &'static str,
    consumer: HeapCons<f32>,
    marks: HeapCons<GapMark>,
    pipeline: Pipeline,
    rates: Arc<R>,
    feed: Feed,
    stats: Arc<CaptureStats>,
    channels: usize,
}

impl<R: Rates> Drain<R> {
    /// What the callback has counted so far, for a source that logs it.
    #[cfg(test)]
    pub(crate) fn stats(&self) -> Arc<CaptureStats> {
        Arc::clone(&self.stats)
    }

    /// Drain until `running` goes false and the ring is empty. `on_samples`
    /// runs after every pop that took samples (the tap logs its buffer
    /// layout from there).
    pub(crate) fn run(self, running: &AtomicBool, mut on_samples: impl FnMut()) {
        let Drain {
            label,
            mut consumer,
            mut marks,
            mut pipeline,
            rates,
            mut feed,
            stats,
            channels,
        } = self;
        let mut silence = Silence::new(channels);
        let mut pending: VecDeque<GapMark> = VecDeque::new();
        let mut consumed: u64 = 0;
        let mut scratch = vec![0.0f32; WORKER_POP_SAMPLES];
        let mut losses = Losses::default();
        loop {
            // Samples first, marks second: a mark is pushed before the
            // samples after it, so every mark these samples need is here.
            let popped = consumer.pop_slice(&mut scratch);
            pending.extend(marks.pop_iter());
            let rate = rates.effective();
            if popped > 0 {
                on_samples();
                pipeline.follow(&*rates, &mut |f: &[i16]| feed.extend(f));
            }
            splice(
                consumed,
                &scratch[..popped],
                &mut pending,
                |piece| match piece {
                    Piece::Samples(samples) => {
                        pipeline.push(samples, &mut |f: &[i16]| feed.extend(f));
                        feed.consumed(samples.len());
                    }
                    Piece::Silence(frames) => {
                        tracing::info!(
                            "{label}: {} ms the device did not deliver, written as silence",
                            frames_to_ns(frames, rate) / 1_000_000
                        );
                        let rest =
                            silence.write(frames, rate, &mut pipeline, &mut |f: &[i16]| {
                                feed.extend(f)
                            });
                        feed.silence(rest, &pipeline);
                    }
                },
            );
            feed.flush(&pipeline);
            losses.warn_new(label, &stats, rate);
            consumed += popped as u64;
            if popped == 0 {
                if !running.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(IDLE_POLL);
            }
        }
        tracing::info!("{label} stopped: {}", stats.describe());
    }
}

/// The ring losses already logged, so each new one is logged once.
#[derive(Default)]
struct Losses {
    dropped_frames: u64,
    lost_marks: u64,
}

impl Losses {
    /// Warn about frames a full ring turned into silence, and gap marks a
    /// full mark ring lost, since the last call.
    fn warn_new(&mut self, label: &str, stats: &CaptureStats, rate: u32) {
        let dropped = stats.dropped_frames.load(Ordering::Relaxed);
        if dropped > self.dropped_frames {
            let new = dropped - self.dropped_frames;
            tracing::warn!(
                "{label}: the capture ring was full; {new} frames ({} ms) were written as \
                 silence instead ({dropped} frames so far)",
                frames_to_ns(new, rate) / 1_000_000
            );
            self.dropped_frames = dropped;
        }
        let lost = stats.lost_marks.load(Ordering::Relaxed);
        if lost > self.lost_marks {
            tracing::warn!(
                "{label}: {} gap marks did not fit their ring; that silence is missing \
                 from the track ({lost} so far)",
                lost - self.lost_marks
            );
            self.lost_marks = lost;
        }
    }
}

/// Writes gap silence: the first part through the resampler, and says how
/// much of the rest is owed straight at 16 kHz ([`Feed::silence`] writes it,
/// timed, TUR-151).
struct Silence {
    channels: usize,
    /// Device-rate zeros, a whole number of frames long.
    device: Vec<f32>,
}

impl Silence {
    fn new(channels: usize) -> Self {
        let channels = channels.max(1);
        Self {
            channels,
            device: vec![0.0; (WORKER_POP_SAMPLES / channels).max(1) * channels],
        }
    }

    /// Push the start of a `frames`-long gap through `pipeline`; returns the
    /// 16 kHz frames of silence still owed for the rest of it.
    fn write(
        &mut self,
        frames: u64,
        rate: u32,
        pipeline: &mut Pipeline,
        sink: &mut impl FnMut(&[i16]),
    ) -> u64 {
        let flush = (u64::from(rate) * FLUSH_THROUGH_MS / 1000).max(FLUSH_THROUGH_MIN_FRAMES);
        let through = frames.min(flush);
        let mut samples = through as usize * self.channels;
        while samples > 0 {
            let n = samples.min(self.device.len());
            pipeline.push(&self.device[..n], sink);
            samples -= n;
        }
        let rest = frames - through;
        ns_to_frames(frames_to_ns(rest, rate), SAMPLE_RATE_HZ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::clock::GapRule;
    use crate::rate_meter::FixedRates;

    const MS: u64 = 1_000_000;

    fn frames_in(path: &std::path::Path) -> u32 {
        hound::WavReader::open(path).unwrap().duration()
    }

    /// Ten 10 ms stereo packets at 48 kHz, every one pushed before the
    /// worker runs, into a ring of `ring` samples; the frames on disk.
    fn record(ring: usize) -> (u32, Arc<CaptureStats>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let track = TrackWriter::open(&path, "test").unwrap();
        let rates = FixedRates::new("test", 48_000);
        let (capture, drain) = ring_sized("test", rates, 2, (track.clone(), None), ring);
        let mut capture = capture.with_gap_rule(GapRule::NEVER);
        let stats = drain.stats();
        let mut packet = vec![0.25f32; 960];
        for n in 0..10 {
            capture.packet(&mut packet, Some(1_000 * MS + n * 10 * MS), false);
        }
        drain.run(&AtomicBool::new(false), || {});
        track.finish().unwrap();
        (frames_in(&path), stats)
    }

    /// TUR-163: a full ring loses no time. What did not fit is written as
    /// silence, so the track is as long as with a ring that held it all.
    #[test]
    fn frames_a_full_ring_dropped_come_back_as_counted_silence() {
        let (whole, whole_stats) = record(1 << 16);
        let (cut, cut_stats) = record(2_001);
        assert_eq!(whole_stats.dropped_frames.load(Ordering::Relaxed), 0);
        // 2 001 samples hold 1 000 whole stereo frames of the 4 800 sent.
        assert_eq!(cut_stats.dropped_frames.load(Ordering::Relaxed), 3_800);
        assert!(whole > 0);
        assert!(
            whole.abs_diff(cut) <= 1,
            "{whole} frames with room for all, {cut} with a full ring"
        );
    }

    #[test]
    fn a_ring_holds_its_seconds_of_the_device_itself() {
        // TUR-163: an 8-channel 96 kHz interface, not 48 kHz stereo.
        assert_eq!(ring_samples(96_000, 8), RING_SECONDS * 96_000 * 8);
        assert_eq!(ring_samples(16_000, 1), RING_SECONDS * 48_000);
    }
}
