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
//! through here instead of being read off the WAV. Both paths do (TUR-164):
//! the batch path ([`crate::transcribe_meeting`]) with the file, the live one
//! ([`crate::SessionOptions::with_timeline`]) with the copy the recorder
//! publishes as it writes it, through the same [`place`].
//!
//! `frame_index / rate` is the nominal rate. Each channel runs on its own
//! device clock, and the mic's crystal and the output device's drift apart
//! (100 ppm is ~270 ms over 45 minutes, past the 200 ms gate). So within a
//! segment the frame is placed through its checkpoint anchors (SPEC A5),
//! which pair a frame with the host time it was captured at: on the straight
//! line between the two anchors around it, and at the nominal rate past the
//! last one (SPEC A37).
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
//!   segment's own clock offset instead of accumulating frame time. The live
//!   tee pads only what *it* drops, so it stays frame-for-frame with the WAV
//!   and goes through this same mapping.
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
//!   one that ordering exists to eliminate. That is why [`SegmentsTimeline::frame_to_sec`]
//!   treats a past-the-end frame as a broken invariant worth logging, not as a
//!   routine short read.
//!
//! Unknown fields are ignored on purpose: `meet-rec` owns this file and may add
//! to it, and a strict parser would turn an additive change into a crash. The
//! per-checkpoint `anchors` array Tess asked for on TUR-4 landed that way, for
//! `drift-check`; since TUR-164 it also corrects the timestamps, and a file
//! without anchors still places every line at the nominal rate.
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
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::segments::{Anchor, Segment, Segments};

/// The `version` this parser was written against (contract §0) — by
/// definition the schema version of the shared type it parses into.
const KNOWN_VERSION: u32 = meeting_format::segments::SCHEMA_VERSION;

/// What transcription reads off `segments.json`: the file itself, and a
/// timestamp for a frame.
///
/// A trait because [`Segments`] lives in `meeting-format`, and Rust only
/// allows inherent methods in the defining crate. Bring it into scope — `use
/// stt::segments::SegmentsTimeline` — and `Segments::read(..)` and
/// `segments.frame_to_sec(..)` read as they did.
pub trait SegmentsTimeline: Sized {
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
    /// the one containing `frame`, then adds that segment's own clock offset
    /// and places the frame inside it through its anchors. Without this, the
    /// unpadded gap at an AirPods swap would silently pull every timestamp
    /// after it earlier than it really happened, and two clocks drifting apart
    /// would pull the two speakers' lines apart over a long meeting.
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

    /// [`Self::frame_to_sec`] for a position given in seconds into the WAV
    /// (`frame / 16000`), which is how the engines report where a line
    /// starts. Fractional, so a line is placed before it is cut to whole
    /// seconds, never after.
    fn wav_sec_to_sec(&self, channel: Channel, wav_sec: f64) -> Option<f64>;
}

impl SegmentsTimeline for Segments {
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
        placed(self, channel, frame as f64)
    }

    fn wav_sec_to_sec(&self, channel: Channel, wav_sec: f64) -> Option<f64> {
        placed(
            self,
            channel,
            wav_sec.max(0.0) * f64::from(meeting_format::SAMPLE_RATE),
        )
    }
}

/// Where a position in one channel's WAV lands on the recording's clock.
///
/// What [`SegmentsTimeline::frame_to_sec`] answers, without its warning:
/// `past_end` says the frame is beyond the frames the segments declare. Live
/// that is routine (the open segment's count is only brought up to date at
/// each checkpoint), so the live path reads this directly; after the
/// recording it means contract §7 broke, which the trait methods log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub sec: f64,
    pub past_end: bool,
}

/// [`Placed`] for `frame`, a (possibly fractional) frame index into the
/// channel's WAV. `None` when the channel has no audio at all.
///
/// The WAV is the concatenation of every segment's frames for that channel,
/// so this walks the segments accumulating frames until it finds the one
/// holding `frame`, then places it inside that segment by its own clock
/// ([`host_offset_ns`]). A segment's own `start_host_ns` is used rather than
/// the frame time of everything before it, so the unpadded gap at a device
/// switch or a pause stays accounted for.
pub(crate) fn place(segments: &Segments, channel: Channel, frame: f64) -> Option<Placed> {
    let origin_ns = segments.start_host_ns()?;
    let mut consumed = 0u64;
    // Last segment that carried audio for this channel, and the frame count
    // that preceded it: the fallback for a frame past the declared total.
    let mut tail: Option<(&Segment, u32, u64)> = None;

    for (i, segment) in segments.segments.iter().enumerate() {
        let rate = segment.rate(channel);
        if rate == 0 {
            continue; // this channel was never captured
        }
        let frames = segment.frames(channel);
        // The final segment's frame count may be unknown if the process was
        // killed before it could be updated; treat it as open-ended rather
        // than refusing to place the timestamp.
        let is_last = i + 1 == segments.segments.len();
        let within = (frame - consumed as f64).max(0.0);
        if within < frames as f64 || (is_last && frames == 0) {
            let sec = sec_at(segment, channel, origin_ns, rate, within);
            return Some(Placed {
                sec,
                past_end: false,
            });
        }
        tail = Some((segment, rate, consumed));
        consumed += frames;
    }

    // Past the last declared frame: extrapolate from the final segment.
    let (segment, rate, before) = tail?;
    let within = (frame - before as f64).max(0.0);
    Some(Placed {
        sec: sec_at(segment, channel, origin_ns, rate, within),
        past_end: true,
    })
}

/// [`place`], logging a frame past the declared total.
///
/// Contract §7 makes that impossible once the recording is over:
/// `segments.json` is renamed into place *before* the WAV headers are patched
/// and the header lengths only grow, so the segments always cover at least as
/// many frames as the file declares, in every crash ordering. Reaching it
/// means that invariant broke, so say so. Extrapolating still beats returning
/// `None` and dropping the line's timestamp, but it is a fallback for a bug,
/// not a supported state.
fn placed(segments: &Segments, channel: Channel, frame: f64) -> Option<f64> {
    let placed = place(segments, channel, frame)?;
    if placed.past_end {
        tracing::warn!(
            ?channel,
            frame,
            declared = segments.total_frames(channel),
            "frame past the last frame segments.json accounts for — the meet-rec \
             frame-count invariant (contract §7) broke; extrapolating the timestamp \
             from the final segment"
        );
    }
    Some(placed.sec)
}

/// Seconds since `origin_ns` for a frame `within` frames into `segment`.
fn sec_at(segment: &Segment, channel: Channel, origin_ns: u64, rate: u32, within: f64) -> f64 {
    let segment_offset_ns = segment.start_host_ns.saturating_sub(origin_ns) as f64;
    (segment_offset_ns + host_offset_ns(segment, channel, rate, within)) / 1e9
}

/// How far a slope between two anchors may sit from the nominal rate and
/// still be believed: a factor of two either way. Real clock error is parts
/// per million; an interval outside this is a broken latch, not a clock.
const BELIEVABLE_SLOPE: std::ops::RangeInclusive<f64> = 0.5..=2.0;

/// Nanoseconds from `segment`'s start to the frame `within` frames into it,
/// on the host clock (SPEC §3.4, TUR-164).
///
/// Nominally `within / rate`. But each channel runs on its own device clock,
/// and two crystals 100 ppm apart slide ~270 ms apart over 45 minutes. The
/// segment's checkpoint anchors record what the host clock said at known
/// frames, so between two anchors the frame is placed on the straight line
/// joining them, and past the last one at the nominal rate from it. The
/// segment's start (frame 0 at `start_host_ns`) is the first point.
///
/// Anchors that do not move both forward, or whose slope is not believable,
/// are skipped rather than trusted: a stalled channel repeats its frames, and
/// an absent one carries `0` host times.
fn host_offset_ns(segment: &Segment, channel: Channel, rate: u32, within: f64) -> f64 {
    let ns_per_frame = 1e9 / f64::from(rate);
    let (mut at_frame, mut at_ns) = (0.0, 0.0);
    for anchor in &segment.anchors {
        let host_ns = anchor.host_ns(channel);
        if host_ns < segment.start_host_ns {
            continue;
        }
        let frame = anchor.frames(channel) as f64;
        let ns = (host_ns - segment.start_host_ns) as f64;
        if frame <= at_frame || ns <= at_ns {
            continue;
        }
        let slope = (ns - at_ns) / (frame - at_frame);
        if !BELIEVABLE_SLOPE.contains(&(slope / ns_per_frame)) {
            continue;
        }
        if within <= frame {
            return at_ns + (within - at_frame) * slope;
        }
        (at_frame, at_ns) = (frame, ns);
    }
    at_ns + (within - at_frame) * ns_per_frame
}

// TUR-164: two clocks drifting apart over a long meeting, placed by anchors.
#[cfg(test)]
mod drift_tests;

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
        // by both IO callbacks). All of it must parse. Since TUR-164 the
        // anchors also place the frames: these say the mic took 5 s of host
        // time for 79999 frames, one short of nominal, so its frame 16000 is
        // a hair after 1 s, and well inside a millisecond of it.
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
        let mic = segments.frame_to_sec(Channel::Mic, 16_000).unwrap();
        let sys = segments.frame_to_sec(Channel::System, 16_000).unwrap();
        assert!((mic - 16_000.0 * 5.0 / 79_999.0).abs() < 1e-9, "mic {mic}");
        assert!(mic > 1.0 && (mic - 1.0).abs() < 1e-3, "mic {mic}");
        assert!((sys - 1.0).abs() < 1e-3, "sys {sys}");
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
        let mic = segments.frame_to_sec(Channel::Mic, 16_000).unwrap();
        assert!((mic - 1.0).abs() < 1e-3, "mic {mic}");
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
