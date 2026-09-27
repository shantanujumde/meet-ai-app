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
//!    See [`Segments::check_wav_header`] for which direction is the safe one
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

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Channel;

/// The `reason` strings a segment can carry.
///
/// `reason` stays a `String` on the wire so a future writer can add one without
/// breaking a reader that has not heard of it (`crates/stt` never branches on
/// it). These are the values `meet-rec` actually emits.
pub mod reason {
    /// The first segment of a recording.
    pub const START: &str = "start";
    /// `kAudioHardwarePropertyDefaultOutputDevice` changed — AirPods in or out.
    pub const DEFAULT_OUTPUT_DEVICE_CHANGED: &str = "default_output_device_changed";
    /// `kAudioHardwarePropertyDefaultInputDevice` changed.
    pub const DEFAULT_INPUT_DEVICE_CHANGED: &str = "default_input_device_changed";
    /// The device kept its identity but changed sample rate or channel count.
    pub const FORMAT_CHANGED: &str = "format_changed";
    /// The machine woke from sleep. See amendment 3 in the module docs: host
    /// time did not advance while it slept, so this boundary is the only place
    /// the lost wall clock is recoverable.
    pub const SYSTEM_WAKE: &str = "system_wake";
    /// An IO proc stopped without a device change — restarted in place.
    pub const STREAM_RESTART: &str = "stream_restart";
}

/// Milliseconds of audio a WAV header declares.
///
/// This is the **only** correct source of a recording's duration. It is not
/// `sum(*_frames)`: after `kill -9` the segments legitimately describe up to one
/// checkpoint more than the headers expose (see [`Segments::check_wav_header`]),
/// so summing overstates by up to [`CHECKPOINT_INTERVAL_S`] seconds and lets a
/// player or a scrubber place a position past the end of the audio that exists.
pub fn duration_ms(wav_header_frames: u64) -> f64 {
    wav_header_frames as f64 * 1000.0 / SAMPLE_RATE_HZ as f64
}

/// Schema version written into every `segments.json`.
///
/// Version 1 is the first shape that ever reaches disk — anchors included — so
/// there is no v0 to migrate.
pub const SCHEMA_VERSION: u32 = 1;

/// The rate of every WAV this crate writes (SPEC §2.3 does the resampling).
pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// SPEC §5 Phase 0 exit gate.
pub const DRIFT_GATE_MS: f64 = 200.0;

/// Seconds between checkpoints: one `segments.json` rewrite, one anchor per
/// channel, two WAV header updates. Bounds worst-case tail loss on `kill -9`.
pub const CHECKPOINT_INTERVAL_S: u64 = 5;

/// How much audio a **deliberately closed** segment may carry past its last
/// anchor before [`Segments::drift`] refuses the recording.
///
/// A segment that is not the last one was closed on purpose — a device change,
/// a format change, a wake — and the writer got to run code on the way out. So
/// it must latch a close anchor there, and the only thing allowed to sit past
/// it is the final ring-buffer drain: the anchor is latched *before* the drain
/// (see [`Segments::check_anchors`]), so the frames the drain flushes land in
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

/// `segments.json` in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segments {
    /// [`SCHEMA_VERSION`]. Defaulted when absent so a hand-written §3.4 fixture
    /// still parses.
    #[serde(default = "default_version")]
    pub version: u32,
    pub segments: Vec<Segment>,
}

fn default_version() -> u32 {
    SCHEMA_VERSION
}

/// One continuous capture run. A new segment starts whenever a stream had to be
/// torn down and restarted — a default-device change, a format change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub idx: u32,
    /// `mach_absolute_time()` scaled to nanoseconds, taken from the IO
    /// callback's `AudioTimeStamp.mHostTime`. Frame 0 of *both* channels
    /// corresponds to this instant: whichever stream came up later is
    /// head-padded with silence, and **those padded frames are counted in
    /// `mic_frames`/`sys_frames` and in every anchor**. They have to be — the
    /// pad stands for real elapsed time, so excluding it would move frame 0 off
    /// `start_host_ns` and put every measurement below out by the pad.
    pub start_host_ns: u64,
    /// Always [`SAMPLE_RATE_HZ`] in a real recording; a WAV header carries one
    /// rate and we write one file per channel. `0` means the channel is absent.
    pub mic_rate: u32,
    /// As `mic_rate`. `0` means the process tap never started.
    pub sys_rate: u32,
    /// Frames of `mic.wav` belonging to this segment, head pad included.
    pub mic_frames: u64,
    /// Frames of `system.wav` belonging to this segment, head pad included.
    pub sys_frames: u64,
    /// Why this segment started. One of [`reason`].
    pub reason: String,
    /// `mach_continuous_time()` scaled to nanoseconds, sampled at the same
    /// instant as `start_host_ns`.
    ///
    /// Continuous time is host time plus the time the machine spent asleep, so
    /// `Δcontinuous − Δhost` across a boundary **is** the sleep duration. This
    /// is the field to difference when you want elapsed wall clock; use
    /// `start_host_ns` only against an [`Anchor`], which is in the same host
    /// domain by construction.
    ///
    /// Optional because a §3.4-shaped fixture predates it, and because a
    /// non-macOS writer may not have the clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_continuous_ns: Option<u64>,
    /// `CLOCK_REALTIME` at the same instant, for rendering a segment against a
    /// human calendar.
    ///
    /// Never difference this for timing: it is the one clock here that can jump
    /// backwards, because NTP steps it — which is precisely why the recorder
    /// does not use it for anything and only writes it down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_unix_ns: Option<u64>,
    /// The hardware rate behind the resampler. Informational; a consumer that
    /// does not care can ignore it, which is what `crates/stt` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_device_rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys_device_rate: Option<u32>,
    /// One entry per checkpoint. Empty in a §3.4-shaped fixture, and empty in
    /// any segment shorter than one checkpoint interval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Anchor>,
}

/// A host-clock reference point, latched in the IO callback.
///
/// Per channel rather than one shared `host_ns`, because the two callbacks fire
/// independently: a shared timestamp would carry up to a buffer period (~10 ms
/// at 48 kHz / 512 frames) of ambiguity on whichever channel did not fire last,
/// for no saving.
///
/// `*_host_ns` is the `mHostTime` of that channel's most recent buffer, and
/// `*_frames` is the WAV-domain frame index *that buffer ends at* — converted
/// through the same exact ratio the resampler uses, not "frames flushed so
/// far". Anchoring on flush position would measure our own writer latency
/// instead of the device clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub mic_host_ns: u64,
    pub mic_frames: u64,
    pub sys_host_ns: u64,
    pub sys_frames: u64,
}

impl Anchor {
    fn host_ns(&self, channel: Channel) -> u64 {
        match channel {
            Channel::Mic => self.mic_host_ns,
            Channel::System => self.sys_host_ns,
        }
    }

    fn frames(&self, channel: Channel) -> u64 {
        match channel {
            Channel::Mic => self.mic_frames,
            Channel::System => self.sys_frames,
        }
    }

    /// How far this channel's captured audio has slid from the host clock, in
    /// milliseconds. Positive = the device clock is running fast (more frames
    /// than wall time accounts for).
    fn drift_ms(&self, channel: Channel, start_host_ns: u64) -> f64 {
        let elapsed_ms = (self.host_ns(channel).saturating_sub(start_host_ns)) as f64 / 1e6;
        let audio_ms = self.frames(channel) as f64 * 1000.0 / SAMPLE_RATE_HZ as f64;
        audio_ms - elapsed_ms
    }
}

impl Segment {
    /// Frames recorded for one channel, head pad included.
    pub fn frames(&self, channel: Channel) -> u64 {
        match channel {
            Channel::Mic => self.mic_frames,
            Channel::System => self.sys_frames,
        }
    }

    /// `0` when the channel never started (SPEC §3.4: a failed tap still writes
    /// `segments.json`).
    pub fn rate(&self, channel: Channel) -> u32 {
        match channel {
            Channel::Mic => self.mic_rate,
            Channel::System => self.sys_rate,
        }
    }

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

impl Segments {
    /// Parse `segments.json`.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Total frames declared for a channel across all segments.
    ///
    /// ⛔ **This is not the recording's duration.** The crash-window invariant
    /// is an inequality — after `kill -9` this sum can exceed what the WAV
    /// header exposes by up to one checkpoint. Use [`duration_ms`] on the
    /// header's frame count instead. This function exists to be compared
    /// *against* a header, in [`Segments::check_wav_header`].
    pub fn total_frames(&self, channel: Channel) -> u64 {
        self.segments.iter().map(|s| s.frames(channel)).sum()
    }

    /// True when the channel never produced audio — a failed tap, or a denied
    /// microphone.
    pub fn channel_absent(&self, channel: Channel) -> bool {
        self.segments.is_empty() || self.segments.iter().all(|s| s.rate(channel) == 0)
    }

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
    pub fn check_wav_header(
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

    /// Measure drift against the host clock.
    pub fn drift(&self) -> Result<DriftReport, DriftError> {
        if self.segments.is_empty() {
            return Err(DriftError::NoSegments);
        }
        for channel in [Channel::Mic, Channel::System] {
            if self.channel_absent(channel) {
                return Err(DriftError::ChannelAbsent(channel));
            }
        }
        self.check_anchors()?;
        if self.segments.iter().all(|s| s.anchors.is_empty()) {
            return Err(DriftError::NoAnchors);
        }
        // Order matters: `NoAnchors` above is the better message for a whole
        // file that predates A5, and coverage would otherwise claim it as a
        // partly-anchored recording.
        self.check_anchor_coverage()?;

        let origin = self.segments[0].start_host_ns;
        let mic = self.channel_drift(Channel::Mic, origin);
        let system = self.channel_drift(Channel::System, origin);

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

    /// What every segment boundary cost, in milliseconds of unrecorded wall
    /// clock, in `idx` order.
    ///
    /// Public and separate from [`Segments::drift`] because a boundary gap is
    /// arithmetic on segment *totals* — it does not depend on anchors at all,
    /// and stays answerable for a recording whose drift `drift()` refuses to
    /// certify. "What did the AirPods swap cost?" is still a fair question
    /// about a file with a coverage hole somewhere else in it.
    pub fn boundary_gaps(&self) -> Vec<BoundaryGap> {
        self.segments.windows(2).map(boundary_gap).collect()
    }

    /// Drift is measured *within* a segment, against that segment's own
    /// `start_host_ns`. A boundary gap is lost wall clock, not clock error, and
    /// folding it into the drift figure would report a device switch as drift.
    fn channel_drift(&self, channel: Channel, origin_host_ns: u64) -> ChannelDrift {
        let mut max_abs_ms = 0.0_f64;
        let mut final_ms = 0.0_f64;
        let mut first_breach = None;
        let mut anchors = 0usize;
        let mut tail_unanchored_ms = 0.0_f64;

        for segment in &self.segments {
            for anchor in &segment.anchors {
                anchors += 1;
                let drift_ms = anchor.drift_ms(channel, segment.start_host_ns);
                final_ms = drift_ms;
                if drift_ms.abs() > max_abs_ms {
                    max_abs_ms = drift_ms.abs();
                }
                if first_breach.is_none() && drift_ms.abs() >= DRIFT_GATE_MS {
                    first_breach = Some(Breach {
                        elapsed_s: anchor.host_ns(channel).saturating_sub(origin_host_ns) as f64
                            / 1e9,
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
    fn check_anchors(&self) -> Result<(), DriftError> {
        for (segment_idx, segment) in self.segments.iter().enumerate() {
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
    fn check_anchor_coverage(&self) -> Result<(), DriftError> {
        let last_idx = self.segments.len() - 1;
        for (segment_idx, segment) in self.segments.iter().enumerate() {
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

/// Fields fixed when a segment opens. Frame counts and anchors accumulate
/// afterward through [`SegmentsWriter`].
pub struct SegmentOpen {
    pub start_host_ns: u64,
    pub start_continuous_ns: Option<u64>,
    pub start_unix_ns: Option<u64>,
    pub mic_rate: u32,
    pub sys_rate: u32,
    pub mic_device_rate: Option<u32>,
    pub sys_device_rate: Option<u32>,
    pub reason: String,
}

/// The write side of `segments.json` — the counterpart to [`Segments`], which
/// only reads. `meet-rec` is the only thing that constructs one.
///
/// Nothing touches disk until [`SegmentsWriter::write_atomic`], which is the
/// §11/§7 checkpoint write: temp file, `fsync`, `rename(2)`. The caller
/// sequences that as the *second* of the checkpoint's three writes — after
/// `WavWriter::fsync_data` on both channels, before `WavWriter::patch_header`
/// on either (see the `wav_writer` module docs and
/// [`Segments::check_wav_header`] for why that order is the safe direction).
pub struct SegmentsWriter {
    segments: Vec<Segment>,
}

impl SegmentsWriter {
    /// Start a new recording with its first segment.
    pub fn new(open: SegmentOpen) -> Self {
        Self {
            segments: vec![Self::segment(0, open)],
        }
    }

    fn segment(idx: u32, open: SegmentOpen) -> Segment {
        Segment {
            idx,
            start_host_ns: open.start_host_ns,
            mic_rate: open.mic_rate,
            sys_rate: open.sys_rate,
            mic_frames: 0,
            sys_frames: 0,
            reason: open.reason,
            start_continuous_ns: open.start_continuous_ns,
            start_unix_ns: open.start_unix_ns,
            mic_device_rate: open.mic_device_rate,
            sys_device_rate: open.sys_device_rate,
            anchors: Vec::new(),
        }
    }

    fn current_mut(&mut self) -> &mut Segment {
        self.segments
            .last_mut()
            .expect("a writer always has at least one segment")
    }

    /// Update the current segment's running frame counts. Call this whenever
    /// frames are appended to either WAV — [`SegmentsWriter::write_atomic`]
    /// just needs the totals to already be current when it runs.
    pub fn update_frames(&mut self, mic_frames: u64, sys_frames: u64) {
        let seg = self.current_mut();
        seg.mic_frames = mic_frames;
        seg.sys_frames = sys_frames;
    }

    /// Append an ordinary checkpoint anchor to the current segment (§11).
    /// `anchor`'s frame counts must be the WAV-domain frame index each
    /// channel's last buffer *ended at* — not "frames flushed so far" (see
    /// the module docs on [`Anchor`]); anchoring on flush position would
    /// measure this writer's own latency instead of the device clock.
    pub fn checkpoint_anchor(&mut self, anchor: Anchor) {
        self.current_mut().anchors.push(anchor);
    }

    /// Close the current segment and open the next one.
    ///
    /// `close_anchor` must be latched *after* the outgoing stream's ring
    /// buffer has been drained — [`CLOSE_ANCHOR_SLACK_MS`] is sized for
    /// exactly that drain. Latch it before draining (or skip it) and
    /// [`Segments::drift`] will correctly refuse the segment later: from the
    /// reader's side, a missing close anchor is indistinguishable from a
    /// writer that never got the chance to run this method, which is exactly
    /// the failure F1 exists to catch.
    pub fn close_segment(&mut self, close_anchor: Anchor, next: SegmentOpen) {
        self.current_mut().anchors.push(close_anchor);
        let idx = self.segments.len() as u32;
        self.segments.push(Self::segment(idx, next));
    }

    /// A read-only snapshot of everything written so far — e.g. to run
    /// [`Segments::check_wav_header`] mid-recording without a round trip
    /// through disk.
    pub fn as_segments(&self) -> Segments {
        Segments {
            version: SCHEMA_VERSION,
            segments: self.segments.clone(),
        }
    }

    /// Render the current state as `segments.json`.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(&self.as_segments())
    }

    /// §7/§11's checkpoint write: a temp file in the same directory, `fsync`,
    /// then `rename(2)` over `path`. The rename is atomic on APFS/HFS+, so a
    /// concurrent reader observes either the old file or the new one, never a
    /// torn one — that is what makes this safe to call on a live recording
    /// [`crates::stt`] might be tailing.
    pub fn write_atomic(&self, path: &Path) -> io::Result<()> {
        let json = self
            .to_json()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("segments.json");
        let tmp_path = dir.join(format!(".{file_name}.tmp.{}", std::process::id()));

        let mut tmp = File::create(&tmp_path)?;
        tmp.write_all(json.as_bytes())?;
        tmp.sync_all()?;
        drop(tmp);
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a segment whose two device clocks are off by the given parts per
    /// million, anchored every [`CHECKPOINT_INTERVAL_S`] seconds.
    fn drifting_segment(duration_s: u64, mic_ppm: f64, sys_ppm: f64) -> Segment {
        let start_host_ns = 1_000_000_000_u64;
        let frames_at = |elapsed_s: f64, ppm: f64| {
            (elapsed_s * SAMPLE_RATE_HZ as f64 * (1.0 + ppm / 1e6)).round() as u64
        };

        let anchors: Vec<Anchor> = (1..=duration_s / CHECKPOINT_INTERVAL_S)
            .map(|k| {
                let elapsed_s = (k * CHECKPOINT_INTERVAL_S) as f64;
                let host_ns = start_host_ns + (elapsed_s * 1e9) as u64;
                Anchor {
                    mic_host_ns: host_ns,
                    mic_frames: frames_at(elapsed_s, mic_ppm),
                    sys_host_ns: host_ns,
                    sys_frames: frames_at(elapsed_s, sys_ppm),
                }
            })
            .collect();

        let last = anchors.last().copied().unwrap();
        Segment {
            idx: 0,
            start_host_ns,
            mic_rate: SAMPLE_RATE_HZ,
            sys_rate: SAMPLE_RATE_HZ,
            mic_frames: last.mic_frames,
            sys_frames: last.sys_frames,
            reason: reason::START.into(),
            start_continuous_ns: Some(start_host_ns),
            start_unix_ns: Some(1_800_000_000_000_000_000),
            mic_device_rate: Some(48_000),
            sys_device_rate: Some(48_000),
            anchors,
        }
    }

    /// Shift a whole segment forward by `host_ns` of host time and `asleep_ns`
    /// of additional wall clock the host clock never saw.
    fn shift(segment: &mut Segment, host_ns: u64, asleep_ns: u64) {
        segment.start_host_ns += host_ns;
        if let Some(continuous) = segment.start_continuous_ns.as_mut() {
            *continuous += host_ns + asleep_ns;
        }
        if let Some(unix) = segment.start_unix_ns.as_mut() {
            *unix += host_ns + asleep_ns;
        }
        for anchor in &mut segment.anchors {
            anchor.mic_host_ns += host_ns;
            anchor.sys_host_ns += host_ns;
        }
    }

    fn segments(segments: Vec<Segment>) -> Segments {
        Segments {
            version: SCHEMA_VERSION,
            segments,
        }
    }

    const FORTY_FIVE_MINUTES_S: u64 = 45 * 60;

    #[test]
    fn a_spec_3_4_shaped_file_without_anchors_still_parses() {
        let json = r#"{"segments":[{"idx":0,"start_host_ns":123456789,"mic_rate":16000,
            "sys_rate":16000,"mic_frames":1307136,"sys_frames":1307136,"reason":"start"}]}"#;

        let parsed = Segments::from_json(json).expect("§3.4 shape must parse");

        assert_eq!(parsed.version, SCHEMA_VERSION);
        assert_eq!(parsed.segments[0].anchors, []);
        assert_eq!(parsed.segments[0].mic_device_rate, None);
        assert_eq!(parsed.segments[0].start_continuous_ns, None);
        assert_eq!(parsed.segments[0].start_unix_ns, None);
    }

    #[test]
    fn round_trips_through_json() {
        let original = segments(vec![drifting_segment(60, 0.0, 0.0)]);

        let json = serde_json::to_string(&original).unwrap();

        assert_eq!(Segments::from_json(&json).unwrap(), original);
    }

    /// The reason anchors exist at all.
    ///
    /// Both device clocks run 100 ppm fast — the same AirPods feeding mic and
    /// output, say. Over 45 minutes that is 270 ms of real slide on *both*
    /// tracks, and a transcript timestamped from it is a quarter of a second
    /// out by the end. Subtracting one frame count from the other reports 0.
    #[test]
    fn common_mode_drift_is_invisible_to_a_track_subtraction_and_caught_by_anchors() {
        let recording = segments(vec![drifting_segment(FORTY_FIVE_MINUTES_S, 100.0, 100.0)]);

        // The number that was available before anchors: frame count minus
        // frame count. Reads as a perfect recording.
        let naive_ms = (recording.total_frames(Channel::Mic) as f64
            - recording.total_frames(Channel::System) as f64)
            * 1000.0
            / SAMPLE_RATE_HZ as f64;
        assert!(naive_ms.abs() < 1.0, "the naive metric read {naive_ms} ms");

        let report = recording.drift().unwrap();

        assert!(
            (report.mic.final_ms - 270.0).abs() < 1.0,
            "mic drift was {} ms, expected ~270",
            report.mic.final_ms
        );
        assert!(
            (report.system.final_ms - 270.0).abs() < 1.0,
            "system drift was {} ms",
            report.system.final_ms
        );
        assert!(!report.passes(DRIFT_GATE_MS));
        assert!(
            report.max_track_skew_ms < 1.0,
            "track-vs-track skew was {} ms — it is supposed to be blind here",
            report.max_track_skew_ms
        );

        // And the curve says *when*, rather than only at minute 45.
        let breach = report.mic.first_breach.expect("gate was crossed");
        assert!(
            (breach.elapsed_s - 2000.0).abs() < CHECKPOINT_INTERVAL_S as f64,
            "crossed at {} s, expected ~2000 s (200 ms at 100 ppm)",
            breach.elapsed_s
        );
    }

    #[test]
    fn a_clean_forty_five_minute_recording_passes_the_gate() {
        let report = segments(vec![drifting_segment(FORTY_FIVE_MINUTES_S, 2.0, -2.0)])
            .drift()
            .unwrap();

        assert!(report.passes(DRIFT_GATE_MS), "{report:?}");
        assert!(
            report.worst_ms() < 10.0,
            "worst was {} ms",
            report.worst_ms()
        );
        assert_eq!(
            report.mic.anchors as u64,
            FORTY_FIVE_MINUTES_S / CHECKPOINT_INTERVAL_S
        );
    }

    /// A run where the tap never started must refuse, not subtract against
    /// zero and report a flattering number.
    #[test]
    fn an_absent_system_track_refuses_instead_of_reporting_zero_drift() {
        let mut only_mic = drifting_segment(600, 0.0, 0.0);
        only_mic.sys_rate = 0;
        only_mic.sys_frames = 0;
        for anchor in &mut only_mic.anchors {
            anchor.sys_frames = 0;
            anchor.sys_host_ns = 0;
        }

        let err = segments(vec![only_mic]).drift().unwrap_err();

        assert_eq!(err, DriftError::ChannelAbsent(Channel::System));
    }

    #[test]
    fn frame_counts_without_anchors_are_not_a_drift_measurement() {
        let mut no_anchors = drifting_segment(600, 50.0, 0.0);
        no_anchors.anchors.clear();

        let err = segments(vec![no_anchors]).drift().unwrap_err();

        assert_eq!(err, DriftError::NoAnchors);
    }

    /// `kill -9` between the `segments.json` rename and the header write. The
    /// benign direction: segments describe more than the header exposes.
    #[test]
    fn a_header_shorter_than_the_segments_is_the_survivable_crash_window() {
        let recording = segments(vec![drifting_segment(600, 0.0, 0.0)]);
        let declared = recording.total_frames(Channel::Mic);

        let unexposed = recording
            .check_wav_header(Channel::Mic, declared - 16_000)
            .expect("segments ahead of the header is allowed");

        assert_eq!(unexposed, 16_000);
    }

    /// The direction the write ordering exists to prevent: audio the reader can
    /// decode but cannot timestamp.
    #[test]
    fn a_header_longer_than_the_segments_violates_the_invariant() {
        let recording = segments(vec![drifting_segment(600, 0.0, 0.0)]);
        let declared = recording.total_frames(Channel::Mic);

        let err = recording
            .check_wav_header(Channel::Mic, declared + 1)
            .unwrap_err();

        assert!(matches!(
            err,
            InvariantViolation::HeaderAheadOfSegments { declared: d, .. } if d == declared
        ));
    }

    #[test]
    fn a_graceful_stop_leaves_the_header_and_the_segments_equal() {
        let recording = segments(vec![drifting_segment(600, 0.0, 0.0)]);
        let declared = recording.total_frames(Channel::System);

        assert_eq!(
            recording
                .check_wav_header(Channel::System, declared)
                .unwrap(),
            0
        );
    }

    /// An AirPods swap costs real wall clock, and SPEC A5 refuses to pad it
    /// with silence. "Survives a device switch" should therefore be a number,
    /// not a yes/no somebody asserted by listening.
    #[test]
    fn a_device_switch_boundary_gap_is_measurable_in_milliseconds() {
        let first = drifting_segment(600, 0.0, 0.0);
        let gap_ms = 420_u64;
        let mut second = drifting_segment(600, 0.0, 0.0);
        second.idx = 1;
        second.reason = reason::DEFAULT_OUTPUT_DEVICE_CHANGED.into();
        let audio_ns = first.mic_frames * 1_000_000_000 / SAMPLE_RATE_HZ as u64;
        shift(&mut second, audio_ns + gap_ms * 1_000_000, 0);

        let report = segments(vec![first, second]).drift().unwrap();

        let gap = &report.boundary_gaps[0];
        assert_eq!(gap.reason, reason::DEFAULT_OUTPUT_DEVICE_CHANGED);
        assert!(
            (gap.mic_ms - gap_ms as f64).abs() < 1.0,
            "mic boundary gap read {} ms, expected {gap_ms}",
            gap.mic_ms
        );
        assert!((gap.sys_ms - gap_ms as f64).abs() < 1.0);
        // A device switch is not a sleep: the two clocks agree on the gap.
        assert!(gap.asleep_ms.unwrap() < 1.0, "{gap:?}");
        // The gap is lost time, not clock error: it must not land in drift.
        assert!(report.passes(DRIFT_GATE_MS), "{report:?}");
    }

    /// Vox's question on the contract: does a lid close start a new segment,
    /// and can a reader tell?
    ///
    /// It does — `reason: "system_wake"` — but the new segment is not enough on
    /// its own. Host time freezes while the machine sleeps, so a twenty-minute
    /// nap advances `start_host_ns` by only the few hundred ms of teardown and
    /// restart. A reader with host time alone would place every line after the
    /// wake twenty minutes early. Continuous time is what makes the sleep a
    /// number instead of a silent hole.
    #[test]
    fn a_sleep_is_a_segment_boundary_and_its_lost_wall_clock_is_recoverable() {
        let first = drifting_segment(600, 0.0, 0.0);
        let restart_ms = 300_u64;
        let asleep_ms = 20 * 60 * 1000_u64;
        let mut second = drifting_segment(600, 0.0, 0.0);
        second.idx = 1;
        second.reason = reason::SYSTEM_WAKE.into();
        let audio_ns = first.mic_frames * 1_000_000_000 / SAMPLE_RATE_HZ as u64;
        shift(
            &mut second,
            audio_ns + restart_ms * 1_000_000,
            asleep_ms * 1_000_000,
        );

        let recording = segments(vec![first, second]);
        let report = recording.drift().unwrap();
        let gap = &report.boundary_gaps[0];

        assert_eq!(gap.reason, reason::SYSTEM_WAKE);
        assert!(
            (gap.asleep_ms.unwrap() - asleep_ms as f64).abs() < 1.0,
            "sleep read as {:?} ms, expected {asleep_ms}",
            gap.asleep_ms
        );
        assert!(
            (gap.mic_ms - (asleep_ms + restart_ms) as f64).abs() < 1.0,
            "unrecorded wall clock read {} ms",
            gap.mic_ms
        );

        // What a host-time-only reader would have seen, and why the field
        // exists: the sleep is invisible and the gap looks like a fast restart.
        let host_only_ms = (recording.segments[1].start_host_ns
            - recording.segments[0].start_host_ns) as f64
            / 1e6
            - recording.segments[0].audio_ms(Channel::Mic);
        assert!(
            (host_only_ms - restart_ms as f64).abs() < 1.0,
            "host-only gap read {host_only_ms} ms"
        );

        // And the sleep must not be charged to the device clocks as drift.
        assert!(report.passes(DRIFT_GATE_MS), "{report:?}");
    }

    /// Vox's other note: nothing downstream may take a duration from the frame
    /// sums, because the crash window makes them an upper bound.
    #[test]
    fn a_crash_truncated_recordings_duration_comes_from_the_header_not_the_sums() {
        let recording = segments(vec![drifting_segment(600, 0.0, 0.0)]);
        let declared = recording.total_frames(Channel::Mic);
        // `kill -9` after the rename, before the header patch: one checkpoint
        // of audio is described by the segments but not exposed by the header.
        let header_frames = declared - SAMPLE_RATE_HZ as u64 * CHECKPOINT_INTERVAL_S;

        let from_header_ms = duration_ms(header_frames);
        let from_sums_ms = duration_ms(declared);

        assert!((from_header_ms - 595_000.0).abs() < 1.0, "{from_header_ms}");
        assert!(
            (from_sums_ms - from_header_ms - 5_000.0).abs() < 1.0,
            "the sums overstate by exactly one checkpoint: {from_sums_ms} vs {from_header_ms}"
        );
        // The overstatement is bounded, and it is the same bound the invariant
        // promises — so a consumer that uses the header can never be asked for
        // a position past the end of the audio that exists.
        assert_eq!(
            recording
                .check_wav_header(Channel::Mic, header_frames)
                .unwrap(),
            SAMPLE_RATE_HZ as u64 * CHECKPOINT_INTERVAL_S
        );
    }

    /// Audio captured after the last anchor is shutdown raggedness. It is
    /// reported, but never as drift.
    #[test]
    fn tail_audio_past_the_last_anchor_is_reported_separately_from_drift() {
        let mut segment = drifting_segment(600, 0.0, 0.0);
        segment.mic_frames += 8_000; // half a second of teardown skew

        let report = segments(vec![segment]).drift().unwrap();

        assert!((report.mic.tail_unanchored_ms - 500.0).abs() < 1.0);
        assert!(report.mic.max_abs_ms < 1.0, "{:?}", report.mic);
        assert!(report.passes(DRIFT_GATE_MS));
    }

    #[test]
    fn an_anchor_claiming_more_frames_than_its_segment_is_rejected() {
        let mut segment = drifting_segment(600, 0.0, 0.0);
        segment.anchors.last_mut().unwrap().mic_frames = segment.mic_frames + 1;

        let err = segments(vec![segment]).drift().unwrap_err();

        assert!(matches!(
            err,
            DriftError::AnchorAheadOfSegment {
                channel: Channel::Mic,
                ..
            }
        ));
    }

    /// **F2 — the gate that cannot fail, one level down.**
    ///
    /// A 45-minute call at 100 ppm ends 270 ms out and must fail the gate.
    /// Anchor only its first five minutes and the anchored prefix reads 30 ms:
    /// honest arithmetic, comfortably passing, and a description of one ninth
    /// of the file. Before the coverage refusal, that 30 ms *was* the gate
    /// number for the whole 45 minutes.
    ///
    /// The second half is what makes this a proof rather than a demo of a new
    /// error. Truncate the audio to exactly what the anchors cover and nothing
    /// about the measurement changes — same anchors, same 30 ms, same pass —
    /// because the measurement was never wrong. What was wrong is that the same
    /// answer came back for two files forty minutes apart in length, which is
    /// the signature of a gate that is not reading the recording.
    #[test]
    fn a_recording_anchored_for_only_its_first_five_minutes_refuses() {
        const ANCHORED_S: u64 = 300;
        let mut partly_anchored = drifting_segment(FORTY_FIVE_MINUTES_S, 100.0, 100.0);
        // The audio stays: 45 minutes of frames, anchors stopping at minute 5.
        partly_anchored
            .anchors
            .truncate((ANCHORED_S / CHECKPOINT_INTERVAL_S) as usize);

        let err = segments(vec![partly_anchored.clone()]).drift().unwrap_err();

        let DriftError::AnchorCoverage {
            segment,
            channel,
            uncovered_ms,
            closed_deliberately,
            ..
        } = err
        else {
            panic!("expected an anchor-coverage refusal, got {err:?}");
        };
        assert_eq!((segment, channel), (0, Channel::Mic));
        assert!(!closed_deliberately, "a single segment is the last segment");
        assert!(
            (uncovered_ms - 2_400_000.0).abs() < 1_000.0,
            "{uncovered_ms} ms unmeasured, expected ~2400 s"
        );

        // Same anchors, same arithmetic — now covering the whole file.
        let mut truncated = partly_anchored;
        let last = *truncated.anchors.last().unwrap();
        truncated.mic_frames = last.mic_frames;
        truncated.sys_frames = last.sys_frames;

        let report = segments(vec![truncated]).drift().unwrap();

        assert!(
            (report.mic.final_ms - 30.0).abs() < 1.0,
            "the anchored prefix measures {} ms, expected ~30 (300 s at 100 ppm)",
            report.mic.final_ms
        );
        assert!(report.passes(DRIFT_GATE_MS), "{report:?}");

        // And the 45-minute recording that number used to stand in for does
        // not pass — which is what the coverage refusal now stops it hiding.
        let whole = segments(vec![drifting_segment(FORTY_FIVE_MINUTES_S, 100.0, 100.0)]);
        assert!(!whole.drift().unwrap().passes(DRIFT_GATE_MS));
    }

    /// **F1 — a close anchor at every deliberate segment close.**
    ///
    /// The writer half is a `meet-rec` call site that does not exist yet; this
    /// is the half a reader can enforce today. A segment that is not the last
    /// one was closed on purpose, with the writer still running, so there is no
    /// excuse for audio past its final anchor beyond one ring-buffer drain.
    ///
    /// The asymmetry is the point, and it is asserted in both directions here:
    /// the *identical* five-second tail is a refusal in segment 0 and fine in
    /// the last segment, because only the last segment can be cut short by
    /// `kill -9`.
    #[test]
    fn a_segment_closed_without_a_close_anchor_refuses_where_a_killed_one_does_not() {
        let ragged = || {
            let mut segment = drifting_segment(600, 0.0, 0.0);
            // Torn down one whole checkpoint after the last anchor that landed,
            // with nothing latched on the way out.
            segment.anchors.pop();
            segment
        };

        // As the last segment: this is what `kill -9` leaves behind.
        segments(vec![ragged()])
            .drift()
            .expect("a ragged tail on the last segment is a crash, not a corrupt file");

        // The same five seconds before a device switch is a writer bug.
        let first = ragged();
        let mut second = drifting_segment(600, 0.0, 0.0);
        second.idx = 1;
        second.reason = reason::DEFAULT_OUTPUT_DEVICE_CHANGED.into();
        let audio_ns = first.mic_frames * 1_000_000_000 / SAMPLE_RATE_HZ as u64;
        shift(&mut second, audio_ns + 400 * 1_000_000, 0);

        let err = segments(vec![first, second]).drift().unwrap_err();

        let DriftError::AnchorCoverage {
            segment,
            uncovered_ms,
            slack_ms,
            closed_deliberately,
            ..
        } = err
        else {
            panic!("expected an anchor-coverage refusal, got {err:?}");
        };
        assert_eq!(segment, 0);
        assert!(closed_deliberately);
        assert_eq!(slack_ms, CLOSE_ANCHOR_SLACK_MS);
        assert!(
            (uncovered_ms - CHECKPOINT_INTERVAL_S as f64 * 1000.0).abs() < 1.0,
            "{uncovered_ms} ms unmeasured before the switch, expected one checkpoint"
        );
        // That same figure sits inside the last segment's slack — so the
        // refusal is about *which* segment it happened in, not about the size.
        assert!(uncovered_ms < FINAL_TAIL_SLACK_MS);
    }

    /// A stubbed host clock: every checkpoint reporting the host time of the
    /// first, while the frames keep arriving.
    ///
    /// `NonMonotonic` never fires — the clock does not go backwards, it just
    /// stops — so before this refusal the file reached the gate maths and blew
    /// it, which reads to whoever is holding the laptop as a hardware fault. It
    /// is not one. Nothing here measured anything.
    #[test]
    fn a_segment_whose_host_clock_never_advances_refuses() {
        let mut frozen = drifting_segment(300, 0.0, 0.0);
        assert_eq!(frozen.anchors.len(), 60, "Tess's fixture shape");
        let stuck_at = frozen.anchors[0].mic_host_ns;
        for anchor in &mut frozen.anchors {
            anchor.mic_host_ns = stuck_at;
            anchor.sys_host_ns = stuck_at;
        }

        let err = segments(vec![frozen]).drift().unwrap_err();

        assert!(
            matches!(
                err,
                DriftError::FrozenClock {
                    segment: 0,
                    anchor: 1,
                    field: "mic_host_ns",
                }
            ),
            "expected a frozen-clock refusal at the first repeat, got {err:?}"
        );
    }

    /// The dangerous sibling: the latch itself is stuck, so host time *and*
    /// frames repeat. Every anchor then differences to the same small offset,
    /// `passes(200)` returns true, and a recording nobody measured certifies
    /// itself.
    #[test]
    fn a_stuck_anchor_latch_refuses_instead_of_certifying_itself() {
        let mut stuck = drifting_segment(300, 0.0, 0.0);
        let first = stuck.anchors[0];
        stuck.anchors.fill(first);

        let err = segments(vec![stuck.clone()]).drift().unwrap_err();
        assert!(matches!(err, DriftError::FrozenClock { .. }), "{err:?}");

        // What it used to report: a fixed offset that never grows, so max and
        // final agree on a number well under the gate and it waves through.
        let drift_ms = first.drift_ms(Channel::Mic, stuck.start_host_ns);
        assert!(
            drift_ms.abs() < DRIFT_GATE_MS,
            "the frozen anchor read {drift_ms} ms — it used to pass the gate"
        );
    }

    /// Coverage is per segment, not per recording: one well-anchored segment
    /// must not cover for a neighbour that has none at all.
    #[test]
    fn a_segment_with_no_anchors_at_all_is_wholly_unmeasured() {
        let mut first = drifting_segment(600, 0.0, 0.0);
        first.anchors.clear();
        let mut second = drifting_segment(600, 0.0, 0.0);
        second.idx = 1;
        second.reason = reason::STREAM_RESTART.into();
        let audio_ns = first.mic_frames * 1_000_000_000 / SAMPLE_RATE_HZ as u64;
        shift(&mut second, audio_ns, 0);

        let err = segments(vec![first, second]).drift().unwrap_err();

        let DriftError::AnchorCoverage {
            segment,
            uncovered_ms,
            ..
        } = err
        else {
            panic!("expected an anchor-coverage refusal, got {err:?}");
        };
        assert_eq!(segment, 0);
        assert!(
            (uncovered_ms - 600_000.0).abs() < 1.0,
            "an unanchored segment is uncovered end to end, not from its last anchor: {uncovered_ms}"
        );
    }

    /// A boundary gap is arithmetic on segment totals and never touches an
    /// anchor, so it stays answerable for a file whose drift is refused.
    #[test]
    fn boundary_gaps_are_readable_from_a_recording_whose_drift_is_refused() {
        let mut first = drifting_segment(600, 0.0, 0.0);
        first.anchors.pop();
        let gap_ms = 400_u64;
        let mut second = drifting_segment(600, 0.0, 0.0);
        second.idx = 1;
        second.reason = reason::DEFAULT_OUTPUT_DEVICE_CHANGED.into();
        let audio_ns = first.mic_frames * 1_000_000_000 / SAMPLE_RATE_HZ as u64;
        shift(&mut second, audio_ns + gap_ms * 1_000_000, 0);
        let recording = segments(vec![first, second]);

        assert!(recording.drift().is_err());

        let gaps = recording.boundary_gaps();
        assert_eq!(gaps.len(), 1);
        assert!(
            (gaps[0].mic_ms - gap_ms as f64).abs() < 1.0,
            "the missing close anchor moved the gap to {} ms",
            gaps[0].mic_ms
        );
    }

    #[test]
    fn an_anchor_series_that_goes_backwards_is_rejected() {
        let mut segment = drifting_segment(600, 0.0, 0.0);
        let len = segment.anchors.len();
        segment.anchors[len - 1].mic_host_ns = segment.anchors[len - 2].mic_host_ns - 1;

        let err = segments(vec![segment]).drift().unwrap_err();

        assert!(matches!(
            err,
            DriftError::NonMonotonic {
                field: "mic_host_ns",
                ..
            }
        ));
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meet-ai-segments-writer-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn open(reason: &str, start_host_ns: u64) -> SegmentOpen {
        SegmentOpen {
            start_host_ns,
            start_continuous_ns: Some(start_host_ns),
            start_unix_ns: Some(1_790_000_000_000_000_000 + start_host_ns),
            mic_rate: SAMPLE_RATE_HZ,
            sys_rate: SAMPLE_RATE_HZ,
            mic_device_rate: Some(48_000),
            sys_device_rate: Some(48_000),
            reason: reason.into(),
        }
    }

    #[test]
    fn a_single_segment_written_to_disk_round_trips_and_measures_clean() {
        let mut writer = SegmentsWriter::new(open(reason::START, 1_000_000_000));
        writer.update_frames(80_000, 80_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 6_000_000_000,
            mic_frames: 80_000,
            sys_host_ns: 6_000_000_000,
            sys_frames: 80_000,
        });

        let path = temp_path("single.json");
        let _ = std::fs::remove_file(&path);
        writer.write_atomic(&path).unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        let parsed = Segments::from_json(&on_disk).unwrap();
        assert_eq!(parsed, writer.as_segments());

        let report = parsed.drift().expect("a fully-anchored segment measures");
        assert!(report.passes(DRIFT_GATE_MS));
        assert_eq!(report.mic.anchors, 1);
    }

    #[test]
    fn write_atomic_overwrites_the_previous_checkpoint_and_leaves_no_temp_file() {
        let mut writer = SegmentsWriter::new(open(reason::START, 0));
        writer.update_frames(10, 10);
        let path = temp_path("overwrite.json");
        let _ = std::fs::remove_file(&path);
        writer.write_atomic(&path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();

        writer.update_frames(20, 20);
        writer.write_atomic(&path).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();

        assert_ne!(first, second, "the checkpoint on disk did not advance");
        assert!(second.contains("\"mic_frames\": 20"));

        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp checkpoint files were left behind: {leftovers:?}"
        );
    }

    #[test]
    fn closing_a_segment_latches_a_close_anchor_and_the_result_is_drift_measurable() {
        let mut writer = SegmentsWriter::new(open(reason::START, 1_000_000_000));
        // 60s of clean audio in segment 0, checkpointed once.
        writer.update_frames(960_000, 960_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 1_000_000_000 + 60_000_000_000,
            mic_frames: 960_000,
            sys_host_ns: 1_000_000_000 + 60_000_000_000,
            sys_frames: 960_000,
        });

        // The device stopped delivering frames right after that last
        // checkpoint. The close anchor is latched at the drain — a few
        // hundred microseconds later, not after the gap — so it still reads
        // as ~0 drift; the 400ms unrecorded gap (SPEC A5 — never padded) is
        // what elapses *between* this anchor and the next segment's open.
        let last_frame_ns = 1_000_000_000 + 60_000_000_000;
        let close_anchor_ns = last_frame_ns + 500_000; // 0.5ms drain latency
        let next_segment_start_ns = close_anchor_ns + 400_000_000; // the gap
        writer.close_segment(
            Anchor {
                mic_host_ns: close_anchor_ns,
                mic_frames: 960_000,
                sys_host_ns: close_anchor_ns,
                sys_frames: 960_000,
            },
            open(reason::DEFAULT_OUTPUT_DEVICE_CHANGED, next_segment_start_ns),
        );
        writer.update_frames(160_000, 160_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: next_segment_start_ns + 10_000_000_000,
            mic_frames: 160_000,
            sys_host_ns: next_segment_start_ns + 10_000_000_000,
            sys_frames: 160_000,
        });

        let segments = writer.as_segments();
        assert_eq!(segments.segments.len(), 2);
        assert_eq!(segments.segments[0].anchors.len(), 2, "checkpoint + close");
        assert_eq!(segments.segments[1].idx, 1);

        let report = segments
            .drift()
            .expect("both segments are fully anchored, including the close anchor");
        assert!(report.passes(DRIFT_GATE_MS));

        let gaps = segments.boundary_gaps();
        assert_eq!(gaps.len(), 1);
        assert!(
            (gaps[0].mic_ms - 400.0).abs() < 1.0,
            "expected the unpadded 400ms gap, got {} ms",
            gaps[0].mic_ms
        );
    }

    #[test]
    fn a_segment_closed_without_latching_a_close_anchor_is_refused_like_a_dropped_marker() {
        // Regression for F1: skipping close_segment's anchor must not silently
        // pass drift() on the segment it abandoned.
        let mut writer = SegmentsWriter::new(open(reason::START, 0));
        writer.update_frames(80_000, 80_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 5_000_000_000,
            mic_frames: 80_000,
            sys_host_ns: 5_000_000_000,
            sys_frames: 80_000,
        });
        // Segment kept recording another 10s past its last anchor before the
        // (hypothetical, buggy) writer tore it down without a close anchor.
        writer.update_frames(240_000, 240_000);

        let idx = 1;
        writer.segments.push(SegmentsWriter::segment(idx, open(reason::STREAM_RESTART, 10_000_000_000)));

        let err = writer.as_segments().drift().unwrap_err();
        assert!(matches!(
            err,
            DriftError::AnchorCoverage {
                segment: 0,
                closed_deliberately: true,
                ..
            }
        ));
    }
}
