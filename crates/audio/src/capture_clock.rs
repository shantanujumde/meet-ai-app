//! When each written frame was captured (TUR-151).
//!
//! A source's `position()` pairs a host time with the frames written so far,
//! and every anchor in `segments.json` is built from those pairs. Until
//! TUR-151 the time was simply the latest callback's, while the frames were
//! whatever the worker had resampled by then: the gap between the two (the
//! ring's backlog plus the resampler's pending input, up to ~64 ms at 16 kHz)
//! differed per channel, so the anchors did too.
//!
//! The capture callback now pushes a [`TimeMark`] (the capture time of a
//! packet's first sample, and where that sample sits in the ring) into a
//! second small ring, the way the loopback's `GapMark` carries silence. The
//! worker counts the samples it takes ([`CaptureClock::consumed`]) and, after
//! each resampled append, works out the capture time of the frame boundary
//! that append ends at: the latest mark at or before the samples consumed,
//! plus the samples since it, minus what the [`Pipeline`] still holds
//! ([`Pipeline::buffered_input_frames`]). [`Feed`] runs those steps for a
//! worker, and [`crate::track::TrackWriter`] latches the time with the frame
//! count in the same append, so `position()` describes one exact frame:
//! frame `frames` (the next one) starts at `host_ns`.

use std::collections::VecDeque;

use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::pipeline::Pipeline;
#[cfg(test)]
use crate::rate_meter::Rates;
use crate::segments::SAMPLE_RATE_HZ;
use crate::tee::Tee;
use crate::track::TrackWriter;

/// Marks the callback can queue before the worker takes them. A mark per
/// callback (about every 10 ms) and a worker that polls every 2 ms; a lost
/// mark only means the time is carried on from the one before.
pub(crate) const MARK_CAPACITY: usize = 512;

/// How far before "now" a capture time may lie and still be believed, when
/// the OS's clock is not known for certain to be the host clock.
pub(crate) const CAPTURE_WINDOW_NS: u64 = 1_000_000_000;

/// How far after "now" a capture time may lie: none, beyond clock jitter.
const FUTURE_SLACK_NS: u64 = 1_000_000;

/// The 16 kHz frames written as gap silence in one append, at most, so a
/// long gap never needs one big buffer.
const SILENCE_PIECE: usize = 4096;

/// The capture time of one packet's first sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimeMark {
    /// Interleaved samples pushed into the ring before this packet's first.
    pub at_sample: u64,
    /// Host time that sample was captured, in ns.
    pub host_ns: u64,
}

/// The pair of halves: [`MarkWriter`] for the callback, [`CaptureClock`] for
/// the worker, for a ring of `channels`-interleaved samples.
pub(crate) fn time_marks(channels: usize) -> (MarkWriter, CaptureClock) {
    let (tx, rx) = HeapRb::<TimeMark>::new(MARK_CAPACITY).split();
    (
        MarkWriter { tx, pushed: 0 },
        CaptureClock {
            rx,
            pending: VecDeque::new(),
            latest: None,
            consumed: 0,
            channels: channels.max(1) as u64,
        },
    )
}

/// The callback's half: no allocation, no lock, never blocks.
pub(crate) struct MarkWriter {
    tx: HeapProd<TimeMark>,
    /// Interleaved samples pushed into the sample ring so far.
    pushed: u64,
}

impl MarkWriter {
    /// Before pushing a packet's samples: its first sample was captured at
    /// `host_ns`. A full mark ring drops the mark.
    pub(crate) fn mark(&mut self, host_ns: u64) {
        let _ = self.tx.try_push(TimeMark {
            at_sample: self.pushed,
            host_ns,
        });
    }

    /// After pushing: `samples` interleaved samples went into the ring.
    pub(crate) fn advance(&mut self, samples: usize) {
        self.pushed += samples as u64;
    }
}

/// The worker's half: the capture time of any sample boundary it has taken.
pub(crate) struct CaptureClock {
    rx: HeapCons<TimeMark>,
    /// Marks for samples the worker has not taken yet.
    pending: VecDeque<TimeMark>,
    /// The last mark at or before `consumed`.
    latest: Option<TimeMark>,
    /// Interleaved samples taken from the ring so far.
    consumed: u64,
    channels: u64,
}

impl CaptureClock {
    /// `samples` more interleaved samples were taken from the ring.
    pub(crate) fn consumed(&mut self, samples: usize) {
        self.consumed += samples as u64;
    }

    /// The capture time of the boundary `buffered_frames` device frames
    /// before the samples taken so far, at `rate`: with the pipeline's
    /// buffered input, the instant the next written 16 kHz frame starts.
    /// 0 (no position) before any packet came with a time.
    pub(crate) fn boundary_ns(&mut self, rate: u32, buffered_frames: f64) -> u64 {
        self.pending.extend(self.rx.pop_iter());
        while let Some(mark) = self.pending.front().copied() {
            if mark.at_sample > self.consumed {
                break;
            }
            self.latest = Some(mark);
            self.pending.pop_front();
        }
        // Before the first timed packet's samples, count back from it.
        let Some(mark) = self.latest.or_else(|| self.pending.front().copied()) else {
            return 0;
        };
        let since = (self.consumed as f64 - mark.at_sample as f64) / self.channels as f64;
        offset_ns(mark.host_ns, since - buffered_frames, rate)
    }
}

/// `host_ns` moved by `frames` at `rate`, never below 1 (0 means "no time").
fn offset_ns(host_ns: u64, frames: f64, rate: u32) -> u64 {
    if rate == 0 {
        return host_ns.max(1);
    }
    let offset = (frames * 1e9 / f64::from(rate)).round() as i128;
    (i128::from(host_ns) + offset).clamp(1, i128::from(u64::MAX)) as u64
}

/// The capture time of a callback's first frame: the OS's, when it gave
/// one and it is believable against `now_ns` ([`plausible_capture_ns`]), or
/// else the callback's own time less the packet it delivered (the packet
/// was captured before the callback could run). Any further device latency
/// is not known here, and is the same for every packet.
pub(crate) fn first_frame_ns(
    capture_ns: Option<u64>,
    now_ns: u64,
    frames: usize,
    rate: u32,
) -> u64 {
    match capture_ns.and_then(|ns| plausible_capture_ns(ns, now_ns)) {
        Some(ns) => ns,
        None => offset_ns(now_ns, -(frames as f64), rate),
    }
}

/// `capture_ns`, when it is a believable capture time against `now_ns` on
/// the same clock: not 0, not in the future, at most [`CAPTURE_WINDOW_NS`]
/// old. A time from an OS clock that turns out not to be the host clock
/// fails this, rather than skewing one channel by the clocks' difference.
pub(crate) fn plausible_capture_ns(capture_ns: u64, now_ns: u64) -> Option<u64> {
    let fresh = capture_ns != 0
        && capture_ns <= now_ns.saturating_add(FUTURE_SLACK_NS)
        && now_ns.saturating_sub(capture_ns) <= CAPTURE_WINDOW_NS;
    fresh.then_some(capture_ns)
}

/// A worker's path from popped samples to timed appends: resample through
/// a [`Pipeline`], then append to the track with the capture time of the
/// boundary the append ends at.
pub(crate) struct Feed {
    track: TrackWriter,
    clock: CaptureClock,
    tee: Option<Tee>,
    /// The 16 kHz frames resampled since the last append.
    out: Vec<i16>,
    zeros: Vec<i16>,
}

impl Feed {
    pub(crate) fn new(track: TrackWriter, clock: CaptureClock, tee: Option<Tee>) -> Self {
        Self {
            track,
            clock,
            tee,
            out: Vec::with_capacity(SILENCE_PIECE),
            zeros: Vec::new(),
        }
    }

    /// One slice taken from the ring: follow `rates`, resample, append. The
    /// workers do this through `loopback::drain` since TUR-163, with gap
    /// silence spliced in; the tests here drive it directly.
    #[cfg(test)]
    pub(crate) fn push(&mut self, pipeline: &mut Pipeline, rates: &impl Rates, samples: &[f32]) {
        let out = &mut self.out;
        let mut sink = |frames: &[i16]| out.extend_from_slice(frames);
        pipeline.follow(rates, &mut sink);
        pipeline.push(samples, &mut sink);
        self.clock.consumed(samples.len());
        self.flush(pipeline);
    }

    /// Keep resampled frames for the next [`Self::flush`].
    pub(crate) fn extend(&mut self, frames: &[i16]) {
        self.out.extend_from_slice(frames);
    }

    /// `samples` more were taken from the ring (see [`CaptureClock::consumed`]).
    pub(crate) fn consumed(&mut self, samples: usize) {
        self.clock.consumed(samples);
    }

    /// Append what was resampled since the last flush, timed by where
    /// `pipeline` now stands.
    pub(crate) fn flush(&mut self, pipeline: &Pipeline) {
        if self.out.is_empty() {
            return;
        }
        let ns = self
            .clock
            .boundary_ns(pipeline.rate(), pipeline.buffered_input_frames());
        self.track.append(&self.out, ns, self.tee.as_ref());
        self.out.clear();
    }

    /// `frames` of 16 kHz silence that end where `pipeline`'s buffered input
    /// begins: the rest of a gap written without resampling it (the
    /// loopback, TUR-37), ahead of the gap's last zeros the pipeline still
    /// holds. What was resampled before it goes first; every append is timed
    /// by where it ends, so none claims a time its frames do not reach.
    pub(crate) fn silence(&mut self, frames: u64, pipeline: &Pipeline) {
        let end = self
            .clock
            .boundary_ns(pipeline.rate(), pipeline.buffered_input_frames());
        let at = |left: u64| {
            if end == 0 {
                0
            } else {
                offset_ns(end, -(left as f64), SAMPLE_RATE_HZ)
            }
        };
        if !self.out.is_empty() {
            self.track.append(&self.out, at(frames), self.tee.as_ref());
            self.out.clear();
        }
        if self.zeros.len() < SILENCE_PIECE {
            self.zeros.resize(SILENCE_PIECE, 0);
        }
        let mut left = frames;
        while left > 0 {
            let piece = left.min(SILENCE_PIECE as u64);
            left -= piece;
            self.track
                .append(&self.zeros[..piece as usize], at(left), self.tee.as_ref());
        }
    }
}

#[cfg(test)]
mod tests;
