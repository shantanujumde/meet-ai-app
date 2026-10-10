use std::path::Path;

use meeting_format::segments::{Anchor, Segment};

use super::*;
use crate::session::{LiveEmitter, NoListener, SessionOptions};
use crate::sink::CollectingSink;
use crate::{MeetingPaths, SttEngine, transcribe_meeting};

/// Two segments: 20 minutes with the system clock 1000 ppm fast and the mic
/// 1000 ppm slow (anchored every 5 s), a 3.6 s gap for a device switch, then
/// 10 minutes on fresh clocks. Enough drift and gap that a line placed at
/// its WAV position lands whole seconds off.
fn recording() -> Segments {
    let segment = |idx: u32, start_ns: u64, secs: u64, mic_ppm: f64, sys_ppm: f64| {
        let frames = |ppm: f64, sec: u64| (sec as f64 * 16_000.0 * (1.0 + ppm / 1e6)) as u64;
        Segment {
            idx,
            start_host_ns: start_ns,
            mic_rate: 16_000,
            sys_rate: 16_000,
            mic_frames: frames(mic_ppm, secs),
            sys_frames: frames(sys_ppm, secs),
            reason: "start".into(),
            start_continuous_ns: None,
            start_unix_ns: None,
            mic_device_rate: None,
            sys_device_rate: None,
            anchors: (1..=secs / 5)
                .map(|k| Anchor {
                    mic_host_ns: start_ns + k * 5_000_000_000,
                    mic_frames: frames(mic_ppm, k * 5),
                    sys_host_ns: start_ns + k * 5_000_000_000,
                    sys_frames: frames(sys_ppm, k * 5),
                })
                .collect(),
        }
    };
    Segments {
        version: 1,
        segments: vec![
            segment(0, 1_000_000_000, 1_200, -1_000.0, 1_000.0),
            segment(1, 1_204_600_000_000, 600, 0.0, 300.0),
        ],
    }
}

/// WAV positions to place: early, late in segment 0, and inside segment 1.
fn positions(segments: &Segments, channel: Channel) -> Vec<f64> {
    let first = segments.segments[0].frames(channel) as f64 / 16_000.0;
    vec![3.25, 61.9, 1_100.4, first + 0.5, first + 299.99]
}

/// Writes one line per position, at that WAV position, on each track.
struct AtPositions(Segments);

impl SttEngine for AtPositions {
    fn name(&self) -> &'static str {
        "at-positions"
    }

    fn transcribe(
        &mut self,
        _wav: &Path,
        speaker: Speaker,
        sink: &mut dyn TranscriptSink,
    ) -> Result<(), Error> {
        for wav_sec in positions(&self.0, channel_of(speaker)) {
            sink.write_at(wav_sec, speaker, format!("{wav_sec}"))?;
        }
        sink.flush()
    }
}

/// `[HH:MM:SS] Speaker: text` back into (seconds, speaker label, text).
fn parse(line: &str) -> (u64, String, String) {
    let (time, rest) = line[1..].split_once("] ").unwrap();
    let (speaker, text) = rest.split_once(": ").unwrap();
    let mut parts = time.split(':').map(|part| part.parse::<u64>().unwrap());
    let (h, m, s) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    (h * 3_600 + m * 60 + s, speaker.into(), text.into())
}

/// The live session's answer for one line, through the emitter every live
/// engine settles its lines with.
fn live_line(timeline: &LiveSegments, speaker: Speaker, wav_sec: f64) -> (u64, f64) {
    let options = SessionOptions::new(speaker).with_timeline(timeline.clone());
    let mut emitter = LiveEmitter::new(&options, Box::new(NoListener));
    let mut sink = CollectingSink::new();
    assert!(emitter.finalize(wav_sec, "line", &mut sink).unwrap());
    let placed = place_live(Some(timeline), speaker, wav_sec);
    (sink.utterances[0].start_sec, placed)
}

#[test]
fn batch_and_live_place_the_same_frame_at_the_same_second() {
    let segments = recording();
    let tmp = tempfile::tempdir().unwrap();
    let paths = MeetingPaths::new(tmp.path());
    std::fs::create_dir_all(paths.audio_dir()).unwrap();
    for channel in [Channel::Mic, Channel::System] {
        std::fs::write(paths.wav(channel), b"").unwrap();
    }
    std::fs::write(
        paths.segments_json(),
        serde_json::to_string(&segments).unwrap(),
    )
    .unwrap();

    transcribe_meeting(&paths, &mut AtPositions(segments.clone())).unwrap();
    let body = std::fs::read_to_string(paths.transcript_md()).unwrap();
    let batch: Vec<_> = body.lines().map(parse).collect();
    assert_eq!(batch.len(), 10);

    let timeline = LiveSegments::new();
    timeline.publish(segments.clone());
    let mut moved = 0;
    for (start_sec, label, text) in batch {
        let speaker = if label == "You" {
            Speaker::You
        } else {
            Speaker::Others
        };
        let wav_sec: f64 = text.parse().unwrap();
        let (live, placed) = live_line(&timeline, speaker, wav_sec);
        assert_eq!(start_sec, live, "{label} at WAV {wav_sec}: batch vs live");
        let by_file = segments
            .wav_sec_to_sec(channel_of(speaker), wav_sec)
            .unwrap();
        assert_eq!(placed, by_file, "the live copy and the file disagree");
        if start_sec != wav_sec as u64 {
            moved += 1;
        }
    }
    // The mapping did something: drift and the gap moved most lines.
    assert!(
        moved >= 6,
        "only {moved} lines moved off their WAV position"
    );
}

#[test]
fn a_live_line_before_the_first_publish_stays_at_its_wav_position() {
    let timeline = LiveSegments::new();
    assert_eq!(place_live(Some(&timeline), Speaker::You, 12.75), 12.75);
    assert_eq!(place_live(None, Speaker::Others, 12.75), 12.75);
    assert_eq!(place_live(None, Speaker::Others, -1.0), 0.0);
}

#[test]
fn a_live_line_past_the_open_segments_count_is_placed_from_it() {
    // Live, the open segment's frame count lags until the next checkpoint.
    // A line beyond it is still placed in that segment, at the nominal rate
    // from its last anchor, not refused or thrown back to the WAV position.
    let mut segments = recording();
    segments.segments.truncate(1);
    let declared = segments.segments[0].frames(Channel::System) as f64 / 16_000.0;
    let timeline = LiveSegments::new();
    timeline.publish(segments);
    let placed = place_live(Some(&timeline), Speaker::Others, declared + 2.0);
    assert!((placed - 1_202.0).abs() < 1e-3, "placed at {placed}");
}

#[test]
fn a_batch_meeting_without_segments_json_keeps_wav_positions() {
    let segments = recording();
    let tmp = tempfile::tempdir().unwrap();
    let paths = MeetingPaths::new(tmp.path());
    std::fs::create_dir_all(paths.audio_dir()).unwrap();
    std::fs::write(paths.wav(Channel::Mic), b"").unwrap();

    transcribe_meeting(&paths, &mut AtPositions(segments.clone())).unwrap();
    let body = std::fs::read_to_string(paths.transcript_md()).unwrap();
    let starts: Vec<u64> = body.lines().map(|line| parse(line).0).collect();
    let wav: Vec<u64> = positions(&segments, Channel::Mic)
        .into_iter()
        .map(|sec| sec as u64)
        .collect();
    assert_eq!(starts, wav);
}
