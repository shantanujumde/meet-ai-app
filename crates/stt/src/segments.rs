//! The `segments.json` clock-truth record (SPEC §3.4).
//!
//! `meet-rec` writes this alongside the two WAVs; the STT side reads it. It is
//! the shared contract between Phase 0 and Phase 1, so the type is not ours:
//! it is `meeting_format::segments`, the one definition the writer uses too, a
//! literal transcription of the shape in SPEC §3.4 rather than a
//! convenient-for-us redesign. This module is the reading of it:
//!
//! ```json
//! {"segments":[{"idx":0,"start_host_ns":123456789,"mic_rate":16000,"sys_rate":16000,
//!               "mic_frames":43560000,"sys_frames":43560000,"reason":"start"}]}
//! ```
//!
//! Timestamps derive from `segment.start_host_ns + frame_index / rate`, never
//! from the wall clock at write time. That is the mitigation for the 🔴
//! clock-drift risk in SPEC §7, and it is why transcript timestamps have to go
//! through here instead of being read off the WAV.
//!
//! Points confirmed with Rune (the `meet-rec` author) on TUR-4, against
//! **revision 2** of the on-disk contract, which this module now encodes rather
//! than assumes:
//!
//! - **`mic_rate`/`sys_rate` are always 16000 in a real recording.** A WAV
//!   header carries one rate and there is one file per channel, so a device
//!   rate change is absorbed by the resampler and never reaches this file. The
//!   field stays per-segment (SPEC shape) but does not vary. The hardware rate
//!   arrives separately in `mic_device_rate`/`sys_device_rate`, which we read
//!   for diagnostics and never use for timing.
//! - **A segment boundary is a real, unpadded gap.** Restarting a tap after a
//!   device change loses a few hundred ms of wall clock and `meet-rec` does not
//!   fill it with silence. The loss shows up only as a jump in the next
//!   segment's `start_host_ns`, which is exactly why `frame_to_sec` adds each
//!   segment's own clock offset instead of accumulating frame time.
//! - **Within a segment, frame 0 of both channels is `start_host_ns`** —
//!   `meet-rec` head-pads whichever stream came up later. So a mic frame and a
//!   system frame at the same index inside one segment are the same instant,
//!   and the two channels can still be walked independently (their tails differ
//!   because the streams are torn down at different moments).
//! - **`start_host_ns` is `mach_absolute_time` converted to ns** via
//!   `mach_timebase_info` — the same clock domain Core Audio stamps the IO
//!   callback with. Monotonic, one domain per file, does not advance across
//!   system sleep.
//! - **`sum(*_frames) >= wav_header_frames`, always** (contract §7). `meet-rec`
//!   fsyncs the PCM, then renames `segments.json`, then patches the WAV headers,
//!   and the header lengths only grow. So the surviving crash state is segments
//!   describing frames the header has not declared yet — frames no reader ever
//!   asks about. The opposite direction, audio the segments do not cover, is the
//!   one that ordering exists to eliminate. That is why [`SegmentsExt::frame_to_sec`]
//!   treats a past-the-end frame as a broken invariant worth logging, not as a
//!   routine short read.
//!
//! Unknown fields are ignored on purpose: `meet-rec` owns this file and may add
//! to it, and a strict parser would turn an additive change into a crash. The
//! per-checkpoint `anchors` array Tess asked for on TUR-4 lands that way — it is
//! for `drift-check`, not for us, and must not break transcription if it ships.
//! `version` is read only to warn when the file is newer than what this parser
//! was written against; a future writer that only *adds* fields must not be
//! turned into a hard failure here.

use std::path::Path;

use crate::{Channel, Error};

/// The file, one segment of it, and one checkpoint anchor.
///
/// The shared schema from `meeting-format` — the same type `meet-rec` writes,
/// re-exported at the path this module used to define its own at. That older
/// struct was a narrower copy (no `anchors`, `version` as `Option`), and the
/// point of sharing is that the two can no longer disagree. The reader's
/// tolerance survived the move: a file without `version`, anchors or the A5
/// clocks still parses, as v1.
pub use meeting_format::segments::{Anchor, Segment, Segments};

/// The `version` this parser was written against (contract §0) — by
/// definition the schema version of the shared type it parses into.
const KNOWN_VERSION: u32 = meeting_format::segments::SCHEMA_VERSION;

/// What transcription reads off `segments.json`: the file itself, and a
/// timestamp for a frame.
///
/// A trait because [`Segments`] lives in `meeting-format`, and Rust only
/// allows inherent methods in the defining crate. Bring it into scope — `use
/// stt::segments::SegmentsExt` — and `Segments::read(..)` and
/// `segments.frame_to_sec(..)` read as they did.
pub trait SegmentsExt: Sized {
    /// Read `segments.json` from a meeting's `audio/` directory.
    ///
    /// A newer `version` than this parser knows is a warning, not an error: the
    /// contract's additive-only rule means an unknown revision still transcribes
    /// correctly, and refusing would strand a recording we can in fact read.
    fn read(path: &Path) -> Result<Self, Error>;

    /// Turn a frame offset within one channel's WAV into seconds since the
    /// start of the recording.
    ///
    /// The WAV is the concatenation of every segment's frames for that
    /// channel, so this walks the segments accumulating frames until it finds
    /// the one containing `frame`, then adds that segment's own clock offset.
    /// Without this, the unpadded gap at an AirPods swap would silently pull
    /// every timestamp after it earlier than it really happened.
    ///
    /// Returns `None` only when this channel has no audio at all — no segments,
    /// or a `*_rate` of 0 in every segment because the tap never started.
    ///
    /// A frame past the last declared one still gets a timestamp, extrapolated
    /// from the final segment, but it also logs a warning. Contract §7 commits
    /// to `sum(*_frames) >= wav_header_frames` in every crash ordering, so this
    /// path is unreachable unless that invariant broke upstream — the clamp is
    /// there to keep a transcript line rather than drop it, not to make the bug
    /// invisible.
    fn frame_to_sec(&self, channel: Channel, frame: u64) -> Option<f64>;
}

impl SegmentsExt for Segments {
    fn read(path: &Path) -> Result<Self, Error> {
        let body = std::fs::read_to_string(path)?;
        let segments: Self = serde_json::from_str(&body)
            .map_err(|e| Error::Segments(format!("{}: {e}", path.display())))?;

        let version = segments.version;
        if version > KNOWN_VERSION {
            tracing::warn!(
                path = %path.display(),
                version,
                known = KNOWN_VERSION,
                "segments.json is newer than this build; reading it with the fields we know"
            );
        }

        Ok(segments)
    }

    fn frame_to_sec(&self, channel: Channel, frame: u64) -> Option<f64> {
        let origin_ns = self.start_host_ns()?;

        let mut consumed = 0u64;
        // Last segment that carried audio for this channel, and the frame count
        // that preceded it — the fallback for a frame past the declared total.
        let mut tail: Option<(&Segment, u32, u64)> = None;

        for (i, segment) in self.segments.iter().enumerate() {
            let rate = segment.rate(channel);
            if rate == 0 {
                continue; // this channel was never captured
            }
            let frames = segment.frames(channel);

            // The final segment's frame count may be unknown if the process was
            // killed before it could be updated; treat it as open-ended rather
            // than refusing to place the timestamp.
            let is_last = i + 1 == self.segments.len();
            let within = frame.saturating_sub(consumed);
            if within < frames || (is_last && frames == 0) {
                return Some(sec_at(segment, origin_ns, rate, within));
            }
            tail = Some((segment, rate, consumed));
            consumed += frames;
        }

        // Past the last declared frame. Contract §7 makes this impossible in a
        // well-behaved recording: `segments.json` is renamed into place *before*
        // the WAV headers are patched and the header lengths only grow, so the
        // segments always cover at least as many frames as the file declares —
        // in every crash ordering, not just a graceful stop. Reaching here means
        // that invariant broke, so say so. Extrapolating from the final segment
        // still beats returning `None` and dropping the line's timestamp, but it
        // is a fallback for a bug, not a supported state.
        let (segment, rate, before) = tail?;
        let declared = self
            .segments
            .iter()
            .map(|segment| segment.frames(channel))
            .sum::<u64>();
        tracing::warn!(
            ?channel,
            frame,
            declared,
            "frame past the last frame segments.json accounts for — the meet-rec \
             frame-count invariant (contract §7) broke; extrapolating the timestamp \
             from the final segment"
        );
        Some(sec_at(
            segment,
            origin_ns,
            rate,
            frame.saturating_sub(before),
        ))
    }
}

/// Seconds since `origin_ns` for a frame `within` frames into `segment`.
///
/// The segment's own clock offset is added rather than the frame time of
/// everything before it, so an unpadded device-switch gap stays accounted for
/// instead of silently pulling every later timestamp earlier.
fn sec_at(segment: &Segment, origin_ns: u64, rate: u32, within: u64) -> f64 {
    let segment_offset_sec = segment.start_host_ns.saturating_sub(origin_ns) as f64 / 1e9;
    segment_offset_sec + within as f64 / rate as f64
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Run `body` with tracing captured, and hand back everything it logged.
    fn captured_logs(body: impl FnOnce()) -> String {
        #[derive(Clone)]
        struct Buffer(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for Buffer {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buffer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, body);

        let logged = buffer.0.lock().unwrap().clone();
        String::from_utf8(logged).expect("tracing writes utf-8")
    }

    const ONE_SEGMENT: &str = r#"{"segments":[
        {"idx":0,"start_host_ns":0,"mic_rate":16000,"sys_rate":16000,
         "mic_frames":960000,"sys_frames":960000,"reason":"start"}]}"#;

    // The AirPods swap SPEC §7 calls out as the silent-failure case, in the
    // shape `meet-rec` actually writes it: both rates stay 16000 because the
    // resampler absorbs the hardware change, and the 400 ms lost to restarting
    // the tap appears only as a jump in segment 1's `start_host_ns`. Segment 0
    // holds 60 s of audio starting at host 1.0 s, so its last frame is at host
    // 61.0 s and segment 1's first frame is at 61.4 s. The system track's
    // shorter tail is the teardown skew Rune warned about.
    const DEVICE_SWITCH: &str = r#"{"segments":[
        {"idx":0,"start_host_ns":1000000000,"mic_rate":16000,"sys_rate":16000,
         "mic_device_rate":48000,"sys_device_rate":48000,
         "mic_frames":960000,"sys_frames":960000,"reason":"start"},
        {"idx":1,"start_host_ns":61400000000,"mic_rate":16000,"sys_rate":16000,
         "mic_device_rate":24000,"sys_device_rate":24000,
         "mic_frames":160000,"sys_frames":156800,
         "reason":"default_output_device_changed"}]}"#;

    // The tap never started: `meet-rec` still writes segments.json, with the
    // system track zeroed out.
    const NO_SYSTEM_TRACK: &str = r#"{"segments":[
        {"idx":0,"start_host_ns":0,"mic_rate":16000,"sys_rate":0,
         "mic_frames":960000,"sys_frames":0,"reason":"start"}]}"#;

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
    fn the_gap_at_a_device_switch_is_not_padded_with_silence() {
        let segments: Segments = serde_json::from_str(DEVICE_SWITCH).unwrap();

        // Frame 16000 is 1 s into segment 0, which itself starts at the origin.
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));

        // Segment 0 holds 960000 mic frames = 60 s of audio. The next frame in
        // the WAV is the first frame of segment 1, which is 60.4 s after the
        // origin on the host clock because restarting the tap lost 400 ms that
        // was never written. Accumulating frame time would say 60.0 and every
        // timestamp for the rest of the meeting would run 400 ms early.
        assert_eq!(segments.frame_to_sec(Channel::Mic, 960_000), Some(60.4));
        assert_eq!(
            segments.frame_to_sec(Channel::Mic, 960_000 + 16_000),
            Some(61.4)
        );
    }

    #[test]
    fn the_two_channels_line_up_inside_a_segment() {
        // meet-rec head-pads whichever stream came up later, so frame 0 of both
        // channels is the segment's start_host_ns. The tails differ — the
        // system stream here is 200 ms shorter — and each channel is walked
        // independently, but the same index inside a segment is the same
        // instant in both.
        let segments: Segments = serde_json::from_str(DEVICE_SWITCH).unwrap();

        for index in [0, 16_000, 960_000, 960_000 + 16_000] {
            assert_eq!(
                segments.frame_to_sec(Channel::Mic, index),
                segments.frame_to_sec(Channel::System, index),
                "channels disagree at frame {index}"
            );
        }
    }

    #[test]
    fn a_frame_past_the_declared_total_still_gets_a_timestamp() {
        // Contract §7 rules this out: `segments.json` lands before the WAV
        // headers are patched and header lengths only grow, so the segments
        // always cover every frame the file declares. Reaching this path means
        // that invariant broke, which `frame_to_sec` now logs. We still return a
        // timestamp — a warned-about approximation beats a dropped line — but
        // this is the bug fallback, not a supported read.
        let segments: Segments = serde_json::from_str(ONE_SEGMENT).unwrap();
        assert_eq!(segments.frame_to_sec(Channel::Mic, 1_000_000), Some(62.5));

        // 11 s past the start of segment 1, which itself starts at 60.4 s.
        let switched: Segments = serde_json::from_str(DEVICE_SWITCH).unwrap();
        let sec = switched
            .frame_to_sec(Channel::Mic, 960_000 + 160_000 + 16_000)
            .unwrap();
        assert!((sec - 71.4).abs() < 1e-9, "got {sec}");
    }

    #[test]
    fn a_missing_system_track_has_no_timestamps_at_all() {
        let segments: Segments = serde_json::from_str(NO_SYSTEM_TRACK).unwrap();
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));
        assert_eq!(segments.frame_to_sec(Channel::System, 0), None);
        assert_eq!(segments.frame_to_sec(Channel::System, 16_000), None);
    }

    #[test]
    fn drift_anchors_and_device_rates_do_not_break_transcription() {
        // Revision 2 of the TUR-4 contract, verbatim in the shape §0 and §11
        // publish: a top-level `version`, per-segment hardware rates we ignore,
        // and per-checkpoint anchors carrying a *separate* host timestamp per
        // channel (`mic_host_ns`/`sys_host_ns` — not the single shared
        // `host_ns` of the earlier proposal, because no one instant is reported
        // by both IO callbacks). None of it is ours to interpret. All of it must
        // parse, and transcription timing must be unchanged by its presence.
        let body = r#"{"version":1,"segments":[
            {"idx":0,"start_host_ns":1000000000,"mic_rate":16000,"sys_rate":16000,
             "mic_device_rate":48000,"sys_device_rate":48000,
             "mic_frames":80000,"sys_frames":80000,"reason":"start",
             "anchors":[
               {"mic_host_ns":1000000000,"mic_frames":0,"sys_host_ns":1000000000,"sys_frames":0},
               {"mic_host_ns":6000000000,"mic_frames":79999,"sys_host_ns":5999875000,"sys_frames":79998}
             ]}]}"#;

        let segments: Segments = serde_json::from_str(body).unwrap();
        assert_eq!(segments.version, 1);
        assert_eq!(segments.segments[0].mic_device_rate, Some(48_000));
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));
        assert_eq!(segments.frame_to_sec(Channel::System, 16_000), Some(1.0));
    }

    #[test]
    fn a_missing_system_track_zeroes_its_anchors_too() {
        // Contract §9: when the tap never starts there is no `system.wav` at
        // all, and every anchor carries `sys_host_ns: 0`/`sys_frames: 0`. We
        // must not mistake those zeros for a track that began at the origin —
        // `sys_rate: 0` is the single field that decides, and it already does.
        let body = r#"{"version":1,"segments":[
            {"idx":0,"start_host_ns":1000000000,"mic_rate":16000,"sys_rate":0,
             "mic_frames":80000,"sys_frames":0,"reason":"start",
             "anchors":[
               {"mic_host_ns":6000000000,"mic_frames":79999,"sys_host_ns":0,"sys_frames":0}
             ]}]}"#;

        let segments: Segments = serde_json::from_str(body).unwrap();
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));
        assert_eq!(segments.frame_to_sec(Channel::System, 0), None);
    }

    #[test]
    fn a_broken_frame_count_invariant_is_warned_about_not_swallowed() {
        // The point of contract §7's ordering is that "audio no segment accounts
        // for" cannot happen. If it ever does, the clamp must surface it —
        // Rune's explicit ask was a warning rather than a silent success, so a
        // real bug in meet-rec shows up instead of being smoothed into a
        // plausible-looking timestamp. Assert the log, not just the number.
        let logged = captured_logs(|| {
            let segments: Segments = serde_json::from_str(ONE_SEGMENT).unwrap();
            assert_eq!(segments.frame_to_sec(Channel::Mic, 1_000_000), Some(62.5));
        });
        assert!(logged.contains("WARN"), "no warning logged; got {logged:?}");
        assert!(
            logged.contains("invariant") && logged.contains("1000000"),
            "warning does not name the invariant and the offending frame: {logged:?}"
        );

        // And the ordinary in-range case stays quiet — a warning that fires on
        // healthy recordings is one nobody reads.
        let quiet = captured_logs(|| {
            let segments: Segments = serde_json::from_str(ONE_SEGMENT).unwrap();
            assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));
        });
        assert!(
            quiet.is_empty(),
            "unexpected log on a healthy read: {quiet:?}"
        );
    }

    #[test]
    fn a_future_contract_revision_still_transcribes() {
        // The contract is additive-only, so a `version` we have never seen is a
        // warning in `read`, not a parse failure. Refusing here would strand a
        // recording we can in fact place timestamps in.
        let body = r#"{"version":99,"segments":[
            {"idx":0,"start_host_ns":0,"mic_rate":16000,"sys_rate":16000,
             "mic_frames":960000,"sys_frames":960000,"reason":"start",
             "something_rune_added_later":{"nested":true}}]}"#;

        let segments: Segments = serde_json::from_str(body).unwrap();
        assert_eq!(segments.version, 99);
        assert_eq!(segments.frame_to_sec(Channel::Mic, 16_000), Some(1.0));
    }
}
