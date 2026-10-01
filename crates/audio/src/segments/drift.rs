//! The drift maths over the shared schema: [`SegmentsDrift`], its report types
//! and the refusals ([`DriftError`]) that stand in for a flattering number.

use crate::Channel;

use super::{
    Anchor, CLOSE_ANCHOR_SLACK_MS, DRIFT_GATE_MS, FINAL_TAIL_SLACK_MS, SAMPLE_RATE_HZ, Segment,
    Segments,
};
#[allow(unused_imports)] // named by intra-doc links only
use super::{CHECKPOINT_INTERVAL_S, SegmentsWriter};

/// The fields [`SegmentsDrift::from_json`] requires that the shared schema
/// defaults. Parsed first and thrown away: its only job is serde's
/// missing-field error. Everything else is ignored here and checked by the
/// real parse.
#[derive(serde::Deserialize)]
struct Strict {
    #[allow(dead_code)]
    segments: Vec<StrictSegment>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct StrictSegment {
    mic_frames: u64,
    sys_frames: u64,
    reason: String,
}

/// Drift arithmetic on one anchor. Private: the trait exists only so the maths
/// below reads as `anchor.drift_ms(..)` over a type this crate does not own.
trait AnchorMaths {
    fn drift_ms(&self, channel: Channel, start_host_ns: u64) -> f64;
}

impl AnchorMaths for Anchor {
    /// How far this channel's captured audio has slid from the host clock, in
    /// milliseconds. Positive = the device clock is running fast (more frames
    /// than wall time accounts for).
    fn drift_ms(&self, channel: Channel, start_host_ns: u64) -> f64 {
        let elapsed_ms = (self.host_ns(channel).saturating_sub(start_host_ns)) as f64 / 1e6;
        let audio_ms = self.frames(channel) as f64 * 1000.0 / SAMPLE_RATE_HZ as f64;
        audio_ms - elapsed_ms
    }
}

/// Coverage arithmetic on one segment. Private, like [`AnchorMaths`].
trait SegmentMaths {
    fn audio_ms(&self, channel: Channel) -> f64;
    fn uncovered_ms(&self, channel: Channel) -> f64;
}

impl SegmentMaths for Segment {
    /// Audio in this segment, in milliseconds.
    fn audio_ms(&self, channel: Channel) -> f64 {
        self.frames(channel) as f64 * 1000.0 / SAMPLE_RATE_HZ as f64
    }

    /// Audio in this segment that no anchor covers: everything after the last
    /// anchor, or the whole segment when it has none.
    ///
    /// This is the only honest answer to "how much of this did we measure?",
    /// and it is what both the coverage refusal and
    /// [`ChannelDrift::tail_unanchored_ms`] are asking for.
    fn uncovered_ms(&self, channel: Channel) -> f64 {
        let anchored_frames = self.anchors.last().map_or(0, |a| a.frames(channel));
        self.frames(channel).saturating_sub(anchored_frames) as f64 * 1000.0 / SAMPLE_RATE_HZ as f64
    }
}

/// Why a drift number could not be produced.
///
/// Every variant is a refusal rather than a zero. A run with no system audio at
/// all must not report a passing drift figure (`sys_frames == 0` subtracts to
/// something flattering), so `drift-check` exits non-zero on all of these.
///
/// Not `Eq`: [`DriftError::AnchorCoverage`] carries milliseconds, and the
/// figure a human needs in the message is worth more than a trait nothing in
/// this crate uses.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DriftError {
    #[error("segments.json contains no segments")]
    NoSegments,

    #[error("the {0:?} track is absent (rate 0) — drift between two tracks is not measurable")]
    ChannelAbsent(Channel),

    #[error(
        "no checkpoint anchors: drift against the host clock is not measurable from frame counts alone"
    )]
    NoAnchors,

    #[error("anchor {anchor} of segment {segment} goes backwards ({field})")]
    NonMonotonic {
        segment: usize,
        anchor: usize,
        field: &'static str,
    },

    #[error(
        "anchor {anchor} of segment {segment} claims {anchor_frames} {channel:?} frames, more than the segment's {segment_frames}"
    )]
    AnchorAheadOfSegment {
        segment: usize,
        anchor: usize,
        channel: Channel,
        anchor_frames: u64,
        segment_frames: u64,
    },

    /// The host clock stopped advancing while frames kept arriving.
    ///
    /// [`DriftError::NonMonotonic`] only catches time going *backwards*, and a
    /// stubbed or stuck latch never does. Checkpoints are
    /// [`CHECKPOINT_INTERVAL_S`] apart, so two anchors in one segment cannot
    /// honestly share a host time.
    ///
    /// This is a separate refusal from a failing gate on purpose. A frozen
    /// latch that repeats its frame count too reads as exactly 0 ms of drift at
    /// every anchor and *certifies* the recording; and even when the number
    /// does explode, "your hardware drifted" sends someone hunting a clock bug
    /// that is not there, where "this measurement is not valid" points at the
    /// recorder.
    #[error(
        "anchor {anchor} of segment {segment} repeats the previous {field} — the host clock is not advancing, so nothing here measures anything"
    )]
    FrozenClock {
        segment: usize,
        anchor: usize,
        field: &'static str,
    },

    /// A segment carries more audio past its last anchor than its slack allows:
    /// the anchors stopped before the recording did.
    ///
    /// Without this, `drift()` measures the anchored prefix and returns that as
    /// the gate number — a 45-minute call anchored for five minutes reports the
    /// five minutes and says nothing about the forty. That is the A5 §1 "gate
    /// that cannot fail" failure mode one level down: the arithmetic is right,
    /// the thing it describes is not the recording.
    ///
    /// Slack is [`CLOSE_ANCHOR_SLACK_MS`] for a deliberately closed segment and
    /// [`FINAL_TAIL_SLACK_MS`] for the last one.
    #[error(
        "segment {segment} carries {uncovered_ms:.0} ms of {channel:?} audio past its last anchor, over the {slack_ms:.0} ms allowed — that audio was never measured against the host clock"
    )]
    AnchorCoverage {
        segment: usize,
        channel: Channel,
        /// Audio after the segment's last anchor — or all of it, when the
        /// segment has no anchors at all.
        uncovered_ms: f64,
        slack_ms: f64,
        /// `true` when this is not the last segment, so the writer had the
        /// chance to latch a close anchor and did not take it. `false` for the
        /// last segment, which `kill -9` can cut short.
        closed_deliberately: bool,
    },
}

/// The first anchor at which a channel crossed a threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Breach {
    /// Seconds from `start_host_ns` of segment 0.
    pub elapsed_s: f64,
    pub drift_ms: f64,
}

/// One channel measured against the host clock.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelDrift {
    pub channel: Channel,
    /// Largest absolute drift seen at any anchor.
    pub max_abs_ms: f64,
    /// Drift at the last anchor. This is the gate number.
    pub final_ms: f64,
    /// Where the curve first crossed the gate, if it did.
    pub first_breach: Option<Breach>,
    /// Audio recorded after the last anchor. Not drift — shutdown raggedness.
    /// Reported separately so it can never be mistaken for the gate number.
    pub tail_unanchored_ms: f64,
    pub anchors: usize,
}

/// Everything `drift-check` prints.
#[derive(Debug, Clone, PartialEq)]
pub struct DriftReport {
    pub mic: ChannelDrift,
    pub system: ChannelDrift,
    /// `|mic_drift - sys_drift|` at its worst. The naive track-against-track
    /// number. Kept only so the report can show it sitting near zero while the
    /// per-channel figures are large — that is the common-mode case, and it is
    /// exactly what a frame-count subtraction would have missed.
    pub max_track_skew_ms: f64,
    /// Unrecorded wall clock at each segment boundary. A device switch or a
    /// sleep is a real gap and is never padded (SPEC A5), so this is what it
    /// cost. One entry per boundary, in `idx` order.
    pub boundary_gaps: Vec<BoundaryGap>,
}

/// What a segment boundary cost, in milliseconds of unrecorded wall clock.
///
/// `(start[n] - start[n-1]) - audio[n-1]`, per channel. Elapsed time comes from
/// [`Segment::start_continuous_ns`] when both segments carry it, because a
/// boundary is exactly where host time can be wrong: a `system_wake` boundary
/// measured in host time reports a gap of ~0 for a machine that was asleep for
/// twenty minutes.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryGap {
    /// The `reason` of the segment that opened at this boundary.
    pub reason: String,
    pub mic_ms: f64,
    pub sys_ms: f64,
    /// How much of the gap was sleep: `Δcontinuous − Δhost`. `None` when either
    /// segment predates [`Segment::start_continuous_ns`], in which case the two
    /// figures above fall back to host time and a sleep is under-reported —
    /// which is why the recorder always writes the field.
    pub asleep_ms: Option<f64>,
}

impl DriftReport {
    /// The Phase 0 gate: *both* channels under `gate_ms` against the host
    /// clock, at every anchor — not just at the end.
    pub fn passes(&self, gate_ms: f64) -> bool {
        self.mic.max_abs_ms < gate_ms && self.system.max_abs_ms < gate_ms
    }

    /// The single number SPEC §5 asks to be reported.
    pub fn worst_ms(&self) -> f64 {
        self.mic.max_abs_ms.max(self.system.max_abs_ms)
    }
}

/// The drift maths, and the invariant check, over the shared schema.
///
/// A trait because [`Segments`] is defined in `meeting-format` (so `stt` reads
/// the same type this crate writes) and Rust only allows inherent methods in
/// the defining crate. Bring it into scope — `use audio::segments::SegmentsDrift`
/// — and `Segments::from_json(..)`, `segments.drift()` and friends read exactly
/// as they did when these were inherent methods.
pub trait SegmentsDrift: Sized {
    /// Parse `segments.json`, strictly.
    ///
    /// The shared schema is tolerant — it defaults a missing `mic_frames`,
    /// `sys_frames` or `reason`, because `stt` would rather place a timestamp
    /// than refuse a recording. Drift maths cannot afford that: a defaulted
    /// `0` frame count reads as a segment with no audio, and the coverage and
    /// boundary figures built on it are nonsense that still looks like a
    /// number. So this refuses such a file with serde's own "missing field"
    /// error, exactly as it did before the schema moved to `meeting-format`.
    fn from_json(json: &str) -> Result<Self, serde_json::Error>;

    /// The crash-window invariant, checked against what a WAV header actually
    /// declares.
    ///
    /// A checkpoint writes in this order: sample bytes, then `segments.json`
    /// (temp file + `rename(2)`), then the WAV headers in place. That ordering
    /// is deliberate, because the two crash windows are not equally bad:
    ///
    /// - header first, crash before the rename → the header declares frames no
    ///   segment accounts for. Every frame past the accumulated total has **no
    ///   timestamp**: real speech gets transcribed and then silently dropped.
    /// - `segments.json` first, crash before the header → segments describe
    ///   frames the header does not expose yet. Nobody ever asks about them,
    ///   because a conforming reader only walks frames the header declares.
    ///
    /// So the invariant is an inequality, and it is the second direction:
    ///
    /// > `sum(*_frames) >= wav_header_frames`, always — with equality after a
    /// > graceful stop.
    ///
    /// Strict equality is not achievable across three non-atomic writes and
    /// should not be asserted.
    fn check_wav_header(
        &self,
        channel: Channel,
        wav_header_frames: u64,
    ) -> Result<u64, InvariantViolation>;

    /// Measure drift against the host clock.
    fn drift(&self) -> Result<DriftReport, DriftError>;

    /// What every segment boundary cost, in milliseconds of unrecorded wall
    /// clock, in `idx` order.
    ///
    /// Public and separate from [`SegmentsDrift::drift`] because a boundary gap is
    /// arithmetic on segment *totals* — it does not depend on anchors at all,
    /// and stays answerable for a recording whose drift `drift()` refuses to
    /// certify. "What did the AirPods swap cost?" is still a fair question
    /// about a file with a coverage hole somewhere else in it.
    fn boundary_gaps(&self) -> Vec<BoundaryGap>;
}

impl SegmentsDrift for Segments {
    fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str::<Strict>(json)?;
        serde_json::from_str(json)
    }

    fn check_wav_header(
        &self,
        channel: Channel,
        wav_header_frames: u64,
    ) -> Result<u64, InvariantViolation> {
        let declared = self.total_frames(channel);
        if wav_header_frames > declared {
            return Err(InvariantViolation::HeaderAheadOfSegments {
                channel,
                wav_header_frames,
                declared,
            });
        }
        Ok(declared - wav_header_frames)
    }

    fn drift(&self) -> Result<DriftReport, DriftError> {
        if self.segments.is_empty() {
            return Err(DriftError::NoSegments);
        }
        for channel in [Channel::Mic, Channel::System] {
            if self.channel_absent(channel) {
                return Err(DriftError::ChannelAbsent(channel));
            }
        }
        check_anchors(self)?;
        if self.segments.iter().all(|s| s.anchors.is_empty()) {
            return Err(DriftError::NoAnchors);
        }
        // Order matters: `NoAnchors` above is the better message for a whole
        // file that predates A5, and coverage would otherwise claim it as a
        // partly-anchored recording.
        check_anchor_coverage(self)?;

        let origin = self.segments[0].start_host_ns;
        let mic = channel_drift(self, Channel::Mic, origin);
        let system = channel_drift(self, Channel::System, origin);

        let max_track_skew_ms = self
            .segments
            .iter()
            .flat_map(|s| {
                s.anchors.iter().map(move |a| {
                    (a.drift_ms(Channel::Mic, s.start_host_ns)
                        - a.drift_ms(Channel::System, s.start_host_ns))
                    .abs()
                })
            })
            .fold(0.0_f64, f64::max);

        Ok(DriftReport {
            mic,
            system,
            max_track_skew_ms,
            boundary_gaps: self.boundary_gaps(),
        })
    }

    fn boundary_gaps(&self) -> Vec<BoundaryGap> {
        self.segments.windows(2).map(boundary_gap).collect()
    }
}

/// Drift is measured *within* a segment, against that segment's own
/// `start_host_ns`. A boundary gap is lost wall clock, not clock error, and
/// folding it into the drift figure would report a device switch as drift.
fn channel_drift(segments: &Segments, channel: Channel, origin_host_ns: u64) -> ChannelDrift {
    let mut max_abs_ms = 0.0_f64;
    let mut final_ms = 0.0_f64;
    let mut first_breach = None;
    let mut anchors = 0usize;
    let mut tail_unanchored_ms = 0.0_f64;

    for segment in &segments.segments {
        for anchor in &segment.anchors {
            anchors += 1;
            let drift_ms = anchor.drift_ms(channel, segment.start_host_ns);
            final_ms = drift_ms;
            if drift_ms.abs() > max_abs_ms {
                max_abs_ms = drift_ms.abs();
            }
            if first_breach.is_none() && drift_ms.abs() >= DRIFT_GATE_MS {
                first_breach = Some(Breach {
                    elapsed_s: anchor.host_ns(channel).saturating_sub(origin_host_ns) as f64 / 1e9,
                    drift_ms,
                });
            }
        }
        if !segment.anchors.is_empty() {
            // ⚠ F4 (TUR-4): this assigns rather than accumulates, so an
            // earlier segment's tail is overwritten by a later one. Left
            // alone deliberately — it is TUR-4's fix, not this one's — but
            // `check_anchor_coverage` now bounds every non-final tail at
            // `CLOSE_ANCHOR_SLACK_MS`, so what this can lose is a quarter
            // of a second rather than the four seconds it used to.
            tail_unanchored_ms = segment.uncovered_ms(channel);
        }
    }

    ChannelDrift {
        channel,
        max_abs_ms,
        final_ms,
        first_breach,
        tail_unanchored_ms,
        anchors,
    }
}

/// Anchors must not go backwards, the host clock must actually *advance*,
/// and no anchor may claim frames the segment itself does not. The writer
/// latches an anchor *before* draining the ring buffer, so the drain can
/// only ever push the segment's count further ahead — an anchor past the
/// segment total means the file is corrupt, not that the recording was cut
/// short.
fn check_anchors(segments: &Segments) -> Result<(), DriftError> {
    for (segment_idx, segment) in segments.segments.iter().enumerate() {
        let mut prev: Option<&Anchor> = None;
        for (anchor_idx, anchor) in segment.anchors.iter().enumerate() {
            if let Some(prev) = prev {
                for (field, now, before, is_host_clock) in [
                    ("mic_host_ns", anchor.mic_host_ns, prev.mic_host_ns, true),
                    ("sys_host_ns", anchor.sys_host_ns, prev.sys_host_ns, true),
                    ("mic_frames", anchor.mic_frames, prev.mic_frames, false),
                    ("sys_frames", anchor.sys_frames, prev.sys_frames, false),
                ] {
                    if now < before {
                        return Err(DriftError::NonMonotonic {
                            segment: segment_idx,
                            anchor: anchor_idx,
                            field,
                        });
                    }
                    // Frames are allowed to repeat — a stalled device
                    // delivering nothing is caught by the drift number
                    // going hugely negative. A repeated *host time* is not:
                    // checkpoints are CHECKPOINT_INTERVAL_S apart, so two
                    // anchors in one segment cannot share one.
                    if is_host_clock && now == before {
                        return Err(DriftError::FrozenClock {
                            segment: segment_idx,
                            anchor: anchor_idx,
                            field,
                        });
                    }
                }
            }
            for channel in [Channel::Mic, Channel::System] {
                if anchor.frames(channel) > segment.frames(channel) {
                    return Err(DriftError::AnchorAheadOfSegment {
                        segment: segment_idx,
                        anchor: anchor_idx,
                        channel,
                        anchor_frames: anchor.frames(channel),
                        segment_frames: segment.frames(channel),
                    });
                }
            }
            prev = Some(anchor);
        }
    }
    Ok(())
}

/// Every millisecond of audio must sit under an anchor, or close enough
/// behind the last one that no clock error could hide in the gap.
///
/// Checked per channel and per segment rather than once over the
/// recording, because a hole in the middle is exactly as unmeasured as a
/// hole at the end, and a per-recording figure would let one well-anchored
/// segment cover for a neighbour that has no anchors at all.
fn check_anchor_coverage(segments: &Segments) -> Result<(), DriftError> {
    let last_idx = segments.segments.len() - 1;
    for (segment_idx, segment) in segments.segments.iter().enumerate() {
        let closed_deliberately = segment_idx != last_idx;
        let slack_ms = if closed_deliberately {
            CLOSE_ANCHOR_SLACK_MS
        } else {
            FINAL_TAIL_SLACK_MS
        };
        for channel in [Channel::Mic, Channel::System] {
            let uncovered_ms = segment.uncovered_ms(channel);
            if uncovered_ms > slack_ms {
                return Err(DriftError::AnchorCoverage {
                    segment: segment_idx,
                    channel,
                    uncovered_ms,
                    slack_ms,
                    closed_deliberately,
                });
            }
        }
    }
    Ok(())
}

/// One boundary, from the pair of segments either side of it.
///
/// Takes a two-element window so the caller cannot accidentally pass segments
/// that are not adjacent — the arithmetic is only meaningful for neighbours.
fn boundary_gap(pair: &[Segment]) -> BoundaryGap {
    let (before, after) = (&pair[0], &pair[1]);

    let host_ms = after.start_host_ns.saturating_sub(before.start_host_ns) as f64 / 1e6;
    let continuous_ms = before
        .start_continuous_ns
        .zip(after.start_continuous_ns)
        .map(|(before, after)| after.saturating_sub(before) as f64 / 1e6);

    // Continuous time when we have it: at a `system_wake` boundary host time is
    // short by however long the machine slept, and that is the whole point of
    // measuring the boundary.
    let elapsed_ms = continuous_ms.unwrap_or(host_ms);

    BoundaryGap {
        reason: after.reason.clone(),
        mic_ms: elapsed_ms - before.audio_ms(Channel::Mic),
        sys_ms: elapsed_ms - before.audio_ms(Channel::System),
        asleep_ms: continuous_ms.map(|continuous_ms| (continuous_ms - host_ms).max(0.0)),
    }
}

/// A `segments.json` that contradicts the audio on disk.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvariantViolation {
    #[error(
        "{channel:?}: the WAV header declares {wav_header_frames} frames but segments account for only {declared} — frames past {declared} have no timestamp and would be silently dropped"
    )]
    HeaderAheadOfSegments {
        channel: Channel,
        wav_header_frames: u64,
        declared: u64,
    },
}

#[cfg(test)]
mod tests;
