//! The `segments.json` on-disk contract, and the drift maths that reads it.
//!
//! SPEC §3.4 defines the file; SPEC §5's Phase 0 gate ("drift < 200ms
//! end-to-end") is what has to be computable *from* it. This module is the
//! single definition of that shape, so `meet-rec` (writer), `drift-check`
//! (reader) and the QA harness cannot drift apart from each other while we
//! argue about clocks.
//!
//! Two things here are amendments to the §3.4 literal, recorded as SPEC A5 and
//! agreed on TUR-4 before any of it was written:
//!
//! 1. **Checkpoint anchors.** A recording with no device switch is a *single*
//!    segment: one `start_host_ns` and two frame counts. The only number
//!    derivable from that is `(mic_frames - sys_frames) / rate`, which compares
//!    the two tracks against *each other* and never against the host clock. If
//!    both devices share a clock — the same AirPods, the same USB interface —
//!    that subtraction reads ≈0 while both tracks slide off wall time together,
//!    and the gate passes on a recording whose timeline is wrong. Each 5 s
//!    checkpoint therefore appends an [`Anchor`]: the IO callback's own host
//!    time paired with the frame count that host time corresponds to, per
//!    channel. Drift then has a host-clock reference, a curve, and a maximum.
//!
//! 2. **The frame-count invariant is an inequality.** `segments.json` and the
//!    two WAV headers are three separate writes; `kill -9` lands between them.
//!    See [`SegmentsDrift::check_wav_header`] for which direction is the safe one
//!    and why the checkpoint writes in the order it does.
//!
//! 3. **Sleep is a segment boundary, and host time alone cannot describe it.**
//!    `mach_absolute_time()` — and therefore `AudioTimeStamp.mHostTime` — stops
//!    while the machine is asleep. A lid closed for twenty minutes advances
//!    `start_host_ns` by roughly nothing, so a reader that only has host time
//!    cannot tell a sleep from an instantaneous restart, and every later
//!    transcript line lands twenty minutes early on the wall clock. So a
//!    wake closes the old segment and opens a new one with
//!    `reason: "system_wake"` ([`reason::SYSTEM_WAKE`]), and each segment
//!    additionally carries [`Segment::start_continuous_ns`] (mach *continuous*
//!    time, which does advance across sleep) and [`Segment::start_unix_ns`]
//!    (wall clock). The difference between the first two is the sleep
//!    duration, exactly — see [`BoundaryGap::asleep_ms`].

mod drift;
mod writer;

pub use drift::{
    BoundaryGap, Breach, ChannelDrift, DriftError, DriftReport, InvariantViolation, SegmentsDrift,
};
pub use writer::{SegmentOpen, SegmentsWriter};

/// The schema itself — [`Segments`], [`Segment`], [`Anchor`], [`reason`] and
/// [`SCHEMA_VERSION`] — lives in `meeting-format`, shared with `stt`'s reader,
/// and is re-exported here at its old paths. What stays in this module is what
/// only the writer and `drift-check` need: the drift maths
/// ([`SegmentsDrift`]), the gate constants, and [`SegmentsWriter`].
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::segments::{Anchor, SCHEMA_VERSION, Segment, Segments, reason};

/// Milliseconds of audio a WAV header declares.
///
/// This is the **only** correct source of a recording's duration. It is not
/// `sum(*_frames)`: after `kill -9` the segments legitimately describe up to one
/// checkpoint more than the headers expose (see [`SegmentsDrift::check_wav_header`]),
/// so summing overstates by up to [`CHECKPOINT_INTERVAL_S`] seconds and lets a
/// player or a scrubber place a position past the end of the audio that exists.
pub fn duration_ms(wav_header_frames: u64) -> f64 {
    wav_header_frames as f64 * 1000.0 / SAMPLE_RATE_HZ as f64
}

/// The rate of every WAV this crate writes (SPEC §2.3 does the resampling).
/// The same number as [`meeting_format::SAMPLE_RATE`], by definition.
///
/// Transitional alias; new code should use `meeting_format::SAMPLE_RATE`.
pub const SAMPLE_RATE_HZ: u32 = meeting_format::SAMPLE_RATE;

/// SPEC §5 Phase 0 exit gate.
pub const DRIFT_GATE_MS: f64 = 200.0;

/// Seconds between checkpoints: one `segments.json` rewrite, one anchor per
/// channel, two WAV header updates. Bounds worst-case tail loss on `kill -9`.
pub const CHECKPOINT_INTERVAL_S: u64 = 5;

/// How much audio a **deliberately closed** segment may carry past its last
/// anchor before [`SegmentsDrift::drift`] refuses the recording.
///
/// A segment that is not the last one was closed on purpose — a device change,
/// a format change, a wake — and the writer got to run code on the way out. So
/// it must latch a close anchor there, and the only thing allowed to sit past
/// it is the final ring-buffer drain: the anchor is latched *before* the drain
/// (see [`check_anchors`]), so the frames the drain flushes land in
/// the segment total without ever reaching an anchor. That is tens of
/// milliseconds of buffer, not seconds.
///
/// ⛔ **This is a writer obligation, enforced here by the reader.** `meet-rec`
/// must latch an anchor at every segment close. Without one, a device switch
/// four seconds after the last ordinary checkpoint leaves four seconds of audio
/// that no anchor ever measured, and the gate reports a number that does not
/// describe it.
pub const CLOSE_ANCHOR_SLACK_MS: f64 = 250.0;

/// How much audio the **last** segment may carry past its last anchor.
///
/// The last segment is the only one `kill -9` can cut short, and a killed
/// process does not get to latch a close anchor. The most it can lose is one
/// checkpoint interval of audio (A5 §3), plus the same drain the close case
/// allows. Anything beyond that is not a ragged shutdown — it is a recording
/// whose anchors stopped while the audio kept going.
pub const FINAL_TAIL_SLACK_MS: f64 = CHECKPOINT_INTERVAL_S as f64 * 1000.0 + CLOSE_ANCHOR_SLACK_MS;
