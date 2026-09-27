//! The `segments.json` clock-truth record (SPEC §3.4).
//!
//! `meet-rec` writes this alongside the two WAVs; the STT side reads it. It is
//! the shared contract between Phase 0 and Phase 1, so this module is a
//! deliberately literal transcription of the shape in SPEC §3.4 rather than a
//! convenient-for-us redesign:
//!
//! ```json
//! {"segments":[{"idx":0,"start_host_ns":123456789,"mic_rate":48000,"sys_rate":48000,
//!               "mic_frames":130713600,"sys_frames":130713600,"reason":"start"}]}
//! ```
//!
//! Timestamps derive from `segment.start_host_ns + frame_index / rate`, never
//! from the wall clock at write time. That is the mitigation for the 🔴
//! clock-drift risk in SPEC §7, and it is why transcript timestamps have to go
//! through here instead of being read off the WAV.
//!
//! Unknown fields are ignored on purpose: `meet-rec` owns this file and may add
//! to it, and a strict parser would turn an additive change into a crash.

use std::path::Path;

use crate::{Channel, Error};

/// The whole file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Segments {
    pub segments: Vec<Segment>,
}

/// One continuous stretch of recording with a single clock reference.
///
/// A new segment starts whenever the device changes mid-call — the AirPods
/// swap case — which is why `reason` exists.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub idx: u32,
    /// Host clock at the first frame of this segment, in nanoseconds.
    pub start_host_ns: u64,
    pub mic_rate: u32,
    pub sys_rate: u32,
    /// Frames written to `mic.wav` during this segment.
    #[serde(default)]
    pub mic_frames: u64,
    /// Frames written to `system.wav` during this segment.
    #[serde(default)]
    pub sys_frames: u64,
    /// Why the segment started: `start`, `default_output_device_changed`, …
    #[serde(default)]
    pub reason: String,
}

impl Segment {
    pub fn rate_for(&self, channel: Channel) -> u32 {
        match channel {
            Channel::Mic => self.mic_rate,
            Channel::System => self.sys_rate,
        }
    }

    pub fn frames_for(&self, channel: Channel) -> u64 {
        match channel {
            Channel::Mic => self.mic_frames,
            Channel::System => self.sys_frames,
        }
    }
}

impl Segments {
    /// Read `segments.json` from a meeting's `audio/` directory.
    pub fn read(path: &Path) -> Result<Self, Error> {
        let body = std::fs::read_to_string(path)?;
        serde_json::from_str(&body).map_err(|e| Error::Segments(format!("{}: {e}", path.display())))
    }

    /// The recording's own start time — segment 0's host clock.
    pub fn start_host_ns(&self) -> Option<u64> {
        self.segments.first().map(|segment| segment.start_host_ns)
    }

    /// Turn a frame offset within one channel's WAV into seconds since the
    /// start of the recording.
    ///
    /// The WAV is the concatenation of every segment's frames for that
    /// channel, so this walks the segments accumulating frames until it finds
    /// the one containing `frame`, then adds that segment's own clock offset.
    /// Without this, a sample-rate change at an AirPods swap would silently
    /// skew every timestamp after it.
    pub fn frame_to_sec(&self, channel: Channel, frame: u64) -> Option<f64> {
        let origin_ns = self.start_host_ns()?;

        let mut consumed = 0u64;
        for segment in &self.segments {
            let rate = segment.rate_for(channel);
            if rate == 0 {
                continue;
            }
            let frames = segment.frames_for(channel);

            // The final segment's frame count may be unknown if the process
            // was killed before it could be updated; treat it as open-ended
            // rather than refusing to place the timestamp.
            let is_last = segment.idx as usize + 1 == self.segments.len();
            let within = frame.checked_sub(consumed)?;
            if within < frames || (is_last && frames == 0) {
                let segment_offset_sec =
                    (segment.start_host_ns.saturating_sub(origin_ns)) as f64 / 1_000_000_000.0;
                return Some(segment_offset_sec + within as f64 / rate as f64);
            }
            consumed += frames;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_SEGMENT: &str = r#"{"segments":[
        {"idx":0,"start_host_ns":0,"mic_rate":16000,"sys_rate":16000,
         "mic_frames":960000,"sys_frames":960000,"reason":"start"}]}"#;

    // Two segments, the second starting 60 s later at a different mic rate —
    // the AirPods swap that SPEC §7 calls out as the silent-failure case.
    const DEVICE_SWITCH: &str = r#"{"segments":[
        {"idx":0,"start_host_ns":1000000000,"mic_rate":16000,"sys_rate":16000,
         "mic_frames":960000,"sys_frames":960000,"reason":"start"},
        {"idx":1,"start_host_ns":61000000000,"mic_rate":48000,"sys_rate":16000,
         "mic_frames":2880000,"sys_frames":960000,
         "reason":"default_output_device_changed"}]}"#;

    #[test]
    fn the_spec_shape_parses() {
        let segments: Segments = serde_json::from_str(ONE_SEGMENT).unwrap();
        assert_eq!(segments.segments.len(), 1);
        assert_eq!(segments.segments[0].reason, "start");
        assert_eq!(segments.start_host_ns(), Some(0));
    }

    #[test]
    fn unknown_fields_do_not_break_the_parser() {
        // meet-rec owns this file and may add to it. An additive change must
        // not become a crash in Phase 1.
        let body = r#"{"segments":[{"idx":0,"start_host_ns":0,"mic_rate":16000,
            "sys_rate":16000,"mic_frames":1,"sys_frames":1,"reason":"start",
            "something_rune_added_later":true}],"version":2}"#;
        assert!(serde_json::from_str::<Segments>(body).is_ok());
    }

    #[test]
    fn a_frame_offset_becomes_seconds() {
        let segments: Segments = serde_json::from_str(ONE_SEGMENT).unwrap();
        assert_eq!(segments.frame_to_sec(Channel::Mic, 0), Some(0.0));
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));
        assert_eq!(segments.frame_to_sec(Channel::Mic, 160_000), Some(10.0));
    }

    #[test]
    fn a_rate_change_mid_recording_does_not_skew_later_timestamps() {
        let segments: Segments = serde_json::from_str(DEVICE_SWITCH).unwrap();

        // Frame 16000 is 1 s into segment 0, which itself starts at the origin.
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));

        // Segment 0 holds 960000 mic frames. The first frame of segment 1 is
        // 60 s later on the host clock, and from there the rate is 48 kHz —
        // so 48000 frames in is 61 s, not 63 s as a naive 16 kHz reading of
        // the concatenated WAV would say.
        assert_eq!(segments.frame_to_sec(Channel::Mic, 960_000), Some(60.0));
        assert_eq!(
            segments.frame_to_sec(Channel::Mic, 960_000 + 48_000),
            Some(61.0)
        );

        // The system track kept its rate across the same switch.
        assert_eq!(
            segments.frame_to_sec(Channel::System, 960_000 + 16_000),
            Some(61.0)
        );
    }

    #[test]
    fn a_frame_past_the_end_has_no_timestamp() {
        let segments: Segments = serde_json::from_str(ONE_SEGMENT).unwrap();
        assert_eq!(segments.frame_to_sec(Channel::Mic, 10_000_000), None);
    }
}
