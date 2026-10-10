//! The `segments.json` schema (SPEC §3.4, amended by SPEC A5).
//!
//! One definition for both sides of the file: `audio` writes it at every
//! checkpoint and measures drift from it, `stt` reads it to place transcript
//! timestamps. Before this module each side had its own struct and they had
//! drifted — the reader had no `anchors` and typed `version` as optional — so
//! a field added by the writer was invisible to the reader's type, and nothing
//! failed to compile when they disagreed.
//!
//! The shape is the writer's, in full. The tolerance is the reader's: a file
//! is accepted if it has the §3.4 core (`segments`, and per segment `idx`,
//! `start_host_ns` and the two rates), so a hand-written §3.4 fixture or a file
//! from before a field existed still parses —
//!
//! * `version` defaults to [`UNVERSIONED_SCHEMA`], `1` (v1 is the first shape
//!   that ever reached disk, so a file without one is a v1 file — whatever
//!   [`SCHEMA_VERSION`] later becomes);
//! * `mic_frames`, `sys_frames` and `reason` default to `0`/`""`, because a
//!   reader placing timestamps would rather extrapolate than refuse a
//!   recording;
//! * everything A5 added (`anchors`, the continuous and wall clocks, the device
//!   rates) defaults to empty/`None`.
//!
//! Unknown fields are ignored on purpose. `meet-rec` owns this file and the
//! contract is additive-only, so a strict parser would turn a new field into a
//! crash in every older reader.
//!
//! The maths lives with its users: drift in `audio::segments`, timestamps in
//! `stt::segments`. What is here is the data and the accessors both need.

use serde::{Deserialize, Serialize};

use crate::Channel;

// TUR-164: the file while it is still being written, for the live transcript.
mod live;
pub use live::LiveSegments;

/// Schema version written into every `segments.json`.
///
/// Version 1 is the first shape that ever reaches disk — anchors included — so
/// there is no v0 to migrate. A reader that sees a larger number reads the
/// fields it knows and warns; the contract's additive-only rule makes that
/// safe.
pub const SCHEMA_VERSION: u32 = 1;

/// The version a file with no `version` field is, forever.
///
/// Not [`SCHEMA_VERSION`]: that is what *this* build writes, and it will move.
/// A file without the field predates it, and that was v1 — reading it as
/// "whatever is current" would silently re-interpret an old file under a newer
/// contract the day the number is bumped.
pub const UNVERSIONED_SCHEMA: u32 = 1;

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
    /// The machine woke from sleep. Host time did not advance while it slept
    /// (`audio::segments` amendment 3), so this boundary is the only place the
    /// lost wall clock is recoverable.
    pub const SYSTEM_WAKE: &str = "system_wake";
    /// An IO proc stopped without a device change — restarted in place.
    pub const STREAM_RESTART: &str = "stream_restart";
    /// The system-audio check that runs during a recording found the tap
    /// delivering only zeros (permission off), so the system track was
    /// dropped and the recording goes on microphone-only (TUR-136).
    pub const SYSTEM_AUDIO_DENIED: &str = "system_audio_denied";
    /// The user paused the recording and then resumed it (TUR-146). Both
    /// channels were stopped for the pause, so nothing was written for it;
    /// the gap shows only as the jump in this segment's `start_host_ns`, the
    /// same way a device switch's does.
    pub const RESUMED_AFTER_PAUSE: &str = "resumed_after_pause";
}

/// `segments.json` in full.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segments {
    /// [`SCHEMA_VERSION`] when written. Defaulted to [`UNVERSIONED_SCHEMA`]
    /// when absent, so a hand-written §3.4 fixture, or a file predating the
    /// field, still parses — as v1.
    #[serde(default = "default_version")]
    pub version: u32,
    pub segments: Vec<Segment>,
}

fn default_version() -> u32 {
    UNVERSIONED_SCHEMA
}

/// One continuous capture run. A new segment starts whenever a stream had to be
/// torn down and restarted — a default-device change (the AirPods swap), a
/// format change, a wake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub idx: u32,
    /// `mach_absolute_time()` scaled to nanoseconds, taken from the IO
    /// callback's `AudioTimeStamp.mHostTime`. Frame 0 of *both* channels
    /// corresponds to this instant: whichever stream came up later is
    /// head-padded with silence, and **those padded frames are counted in
    /// `mic_frames`/`sys_frames` and in every anchor**. They have to be — the
    /// pad stands for real elapsed time, so excluding it would move frame 0 off
    /// `start_host_ns` and put every measurement out by the pad.
    pub start_host_ns: u64,
    /// Always [`crate::SAMPLE_RATE`] in a real recording; a WAV header carries
    /// one rate and we write one file per channel. `0` means the channel is
    /// absent.
    pub mic_rate: u32,
    /// As `mic_rate`. `0` means the process tap never started.
    pub sys_rate: u32,
    /// Frames of `mic.wav` belonging to this segment, head pad included.
    ///
    /// Defaulted so a reader can still place timestamps in a file whose last
    /// segment was never updated before the process was killed.
    #[serde(default)]
    pub mic_frames: u64,
    /// Frames of `system.wav` belonging to this segment, head pad included.
    #[serde(default)]
    pub sys_frames: u64,
    /// Why this segment started. One of [`reason`].
    #[serde(default)]
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
    /// does not care can ignore it, which is what `crates/stt` does. Never use
    /// it for timing.
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
    /// This channel's host time at the anchor.
    pub fn host_ns(&self, channel: Channel) -> u64 {
        match channel {
            Channel::Mic => self.mic_host_ns,
            Channel::System => self.sys_host_ns,
        }
    }

    /// This channel's WAV-domain frame index at the anchor.
    pub fn frames(&self, channel: Channel) -> u64 {
        match channel {
            Channel::Mic => self.mic_frames,
            Channel::System => self.sys_frames,
        }
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
}

impl Segments {
    /// The recording's own start time — segment 0's host clock.
    pub fn start_host_ns(&self) -> Option<u64> {
        self.segments.first().map(|segment| segment.start_host_ns)
    }

    /// Total frames declared for a channel across all segments.
    ///
    /// ⛔ **This is not the recording's duration.** The crash-window invariant
    /// is an inequality — after `kill -9` this sum can exceed what the WAV
    /// header exposes by up to one checkpoint. Use `audio::segments::duration_ms`
    /// on the header's frame count instead. This exists to be compared
    /// *against* a header (`audio`'s `check_wav_header`).
    pub fn total_frames(&self, channel: Channel) -> u64 {
        self.segments.iter().map(|s| s.frames(channel)).sum()
    }

    /// True when the channel never produced audio — a failed tap, or a denied
    /// microphone.
    pub fn channel_absent(&self, channel: Channel) -> bool {
        self.segments.is_empty() || self.segments.iter().all(|s| s.rate(channel) == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_spec_3_4_shape_parses_as_v1() {
        let parsed: Segments = serde_json::from_str(
            r#"{"segments":[{"idx":0,"start_host_ns":123456789,"mic_rate":16000,
                "sys_rate":16000,"mic_frames":43560000,"sys_frames":43560000,
                "reason":"start"}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.version, 1);
        assert!(parsed.segments[0].anchors.is_empty());
        assert_eq!(parsed.segments[0].start_continuous_ns, None);
    }

    #[test]
    fn a_file_without_a_version_is_v1_not_whatever_is_current() {
        // Pinned to the literal: this must not follow SCHEMA_VERSION when it
        // is bumped.
        assert_eq!(UNVERSIONED_SCHEMA, 1);
        assert_eq!(default_version(), 1);
        let parsed: Segments = serde_json::from_str(r#"{"segments":[]}"#).unwrap();
        assert_eq!(parsed.version, 1);
    }

    #[test]
    fn a_segment_missing_its_counts_and_reason_still_parses() {
        // The reader's tolerance: a last segment `kill -9` cut short before its
        // counts were written must still place timestamps, not refuse.
        let parsed: Segments = serde_json::from_str(
            r#"{"segments":[{"idx":0,"start_host_ns":0,"mic_rate":16000,"sys_rate":0}]}"#,
        )
        .unwrap();
        let segment = &parsed.segments[0];
        assert_eq!((segment.mic_frames, segment.sys_frames), (0, 0));
        assert_eq!(segment.reason, "");
        assert!(parsed.channel_absent(Channel::System));
    }

    #[test]
    fn unknown_fields_and_newer_versions_are_read_not_refused() {
        let parsed: Segments = serde_json::from_str(
            r#"{"version":99,"segments":[{"idx":0,"start_host_ns":0,"mic_rate":16000,
                "sys_rate":16000,"mic_frames":1,"sys_frames":1,"reason":"start",
                "something_added_later":{"nested":true}}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.version, 99);
    }

    #[test]
    fn the_optional_a5_fields_are_left_out_when_empty() {
        let segments = Segments {
            version: SCHEMA_VERSION,
            segments: vec![Segment {
                idx: 0,
                start_host_ns: 1,
                mic_rate: crate::SAMPLE_RATE,
                sys_rate: 0,
                mic_frames: 2,
                sys_frames: 0,
                reason: reason::START.into(),
                start_continuous_ns: None,
                start_unix_ns: None,
                mic_device_rate: None,
                sys_device_rate: None,
                anchors: Vec::new(),
            }],
        };
        let json = serde_json::to_string(&segments).unwrap();
        assert_eq!(
            json,
            r#"{"version":1,"segments":[{"idx":0,"start_host_ns":1,"mic_rate":16000,"sys_rate":0,"mic_frames":2,"sys_frames":0,"reason":"start"}]}"#
        );
        assert_eq!(serde_json::from_str::<Segments>(&json).unwrap(), segments);
    }
}
