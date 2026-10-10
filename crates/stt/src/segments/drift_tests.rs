//! TUR-164: the mic and the output device run on two crystals, and over a long
//! meeting their audio slides apart. These build the anchors a recorder would
//! write for such a pair and check that placing frames through them keeps both
//! speakers on the host clock, inside SPEC §5's 200 ms gate.

use super::*;

/// SPEC §5's Phase 0 gate, in seconds.
const GATE_SEC: f64 = 0.200;
/// Where the recording starts on the host clock: anything but zero, so an
/// offset that forgets the origin shows.
const START_NS: u64 = 7_000_000_000;
const FORTY_FIVE_MIN_SEC: f64 = 45.0 * 60.0;
/// A5's checkpoint interval.
const CHECKPOINT_SEC: f64 = 5.0;

/// A channel whose clock runs at `ppm` parts per million off the host's: it
/// writes `16000 * (1 + ppm/1e6)` frames per host second.
#[derive(Clone, Copy)]
struct Clock {
    ppm: f64,
}

impl Clock {
    fn frames_at(self, host_sec: f64) -> f64 {
        host_sec * 16_000.0 * (1.0 + self.ppm / 1e6)
    }
}

/// Up to ±2 ms of latch noise on an anchor's host time, the same every run.
fn jitter_ns(k: u64, salt: u64) -> i64 {
    ((k * 7_919 + salt * 104_729) % 41) as i64 * 100_000 - 2_000_000
}

/// One segment of `secs` starting `start_ns`, the two channels on `mic` and
/// `sys`, anchored every checkpoint and once at the close, as A5 writes them.
fn segment(idx: u32, start_ns: u64, secs: f64, mic: Clock, sys: Clock) -> Segment {
    let mut anchors = Vec::new();
    let checkpoints = (secs / CHECKPOINT_SEC).floor() as u64;
    for k in 1..=checkpoints + 1 {
        let at = (k as f64 * CHECKPOINT_SEC).min(secs);
        let host = |salt| (start_ns as i64 + (at * 1e9) as i64 + jitter_ns(k, salt)) as u64;
        anchors.push(Anchor {
            mic_host_ns: host(1),
            mic_frames: mic.frames_at(at) as u64,
            sys_host_ns: host(2),
            sys_frames: sys.frames_at(at) as u64,
        });
        if at >= secs {
            break;
        }
    }
    Segment {
        idx,
        start_host_ns: start_ns,
        mic_rate: 16_000,
        sys_rate: 16_000,
        mic_frames: mic.frames_at(secs) as u64,
        sys_frames: sys.frames_at(secs) as u64,
        reason: if idx == 0 {
            "start"
        } else {
            "default_output_device_changed"
        }
        .into(),
        start_continuous_ns: None,
        start_unix_ns: None,
        mic_device_rate: Some(48_000),
        sys_device_rate: Some(48_000),
        anchors,
    }
}

fn recording(segments: Vec<Segment>) -> Segments {
    Segments {
        version: 1,
        segments,
    }
}

/// The worst error, in seconds, over a moment every ten seconds of a single
/// segment, for one channel on `clock`.
fn worst_error(segments: &Segments, channel: Channel, clock: Clock, secs: f64) -> f64 {
    let mut worst: f64 = 0.0;
    let mut at = 0.0;
    while at <= secs {
        let frame = clock.frames_at(at).floor() as u64;
        let placed = segments.frame_to_sec(channel, frame).unwrap();
        // The frame was captured at `frame`'s own instant, a hair before `at`.
        let truth = frame as f64 / (16_000.0 * (1.0 + clock.ppm / 1e6));
        worst = worst.max((placed - truth).abs());
        at += 10.0;
    }
    worst
}

#[test]
fn a_45_minute_pair_of_clocks_1_001_apart_stays_inside_the_gate() {
    // A relative clock ratio of 1.001: each side 500 ppm off the host, in
    // opposite directions. Ten times the 50-100 ppm a built-in mic and
    // AirPods really show, so the margin is the point.
    let (mic, sys) = (Clock { ppm: -500.0 }, Clock { ppm: 500.0 });
    let segments = recording(vec![segment(0, START_NS, FORTY_FIVE_MIN_SEC, mic, sys)]);

    let mic_error = worst_error(&segments, Channel::Mic, mic, FORTY_FIVE_MIN_SEC);
    let sys_error = worst_error(&segments, Channel::System, sys, FORTY_FIVE_MIN_SEC);
    assert!(
        mic_error < GATE_SEC,
        "mic is {mic_error} s off the host clock"
    );
    assert!(
        sys_error < GATE_SEC,
        "system is {sys_error} s off the host clock"
    );
    // The anchors' own latch noise is the only error left.
    assert!(
        mic_error.max(sys_error) < 0.003,
        "{mic_error} / {sys_error}"
    );

    // The same instant, heard on both tracks at minute 45, lands on the same
    // second of the transcript.
    let end = FORTY_FIVE_MIN_SEC - 1.0;
    let you = segments.frame_to_sec(Channel::Mic, mic.frames_at(end) as u64);
    let others = segments.frame_to_sec(Channel::System, sys.frames_at(end) as u64);
    let (you, others) = (you.unwrap(), others.unwrap());
    assert!(
        (you - others).abs() < GATE_SEC,
        "you {you} vs others {others}"
    );
}

#[test]
fn without_anchors_the_same_pair_misses_the_gate() {
    // The test above has teeth: the nominal-rate rule SPEC §3.4 started with
    // puts the system track 1.35 s late by minute 45.
    let (mic, sys) = (Clock { ppm: -500.0 }, Clock { ppm: 500.0 });
    let mut unanchored = segment(0, START_NS, FORTY_FIVE_MIN_SEC, mic, sys);
    unanchored.anchors.clear();
    let segments = recording(vec![unanchored]);
    let sys_error = worst_error(&segments, Channel::System, sys, FORTY_FIVE_MIN_SEC);
    assert!(sys_error > 1.0, "nominal rate only {sys_error} s off");
}

#[test]
fn a_device_switch_mid_meeting_keeps_its_gap_and_its_own_drift() {
    // 20 minutes on the built-in speakers, a 400 ms gap for the AirPods swap,
    // then 25 minutes with the output on the buds' clock. Each segment is
    // placed by its own start and its own anchors.
    let mic = Clock { ppm: -40.0 };
    let (speakers, buds) = (Clock { ppm: 30.0 }, Clock { ppm: 95.0 });
    let first_secs = 20.0 * 60.0;
    let second_start = START_NS + (first_secs * 1e9) as u64 + 400_000_000;
    let segments = recording(vec![
        segment(0, START_NS, first_secs, mic, speakers),
        segment(1, second_start, 25.0 * 60.0, mic, buds),
    ]);

    let before_sys = segments.segments[0].sys_frames;
    let second_offset = (second_start - START_NS) as f64 / 1e9;
    for at in [0.0, 60.0, 600.0, 1_499.0] {
        let frame = before_sys + buds.frames_at(at) as u64;
        let placed = segments.frame_to_sec(Channel::System, frame).unwrap();
        let truth = second_offset + at;
        assert!((placed - truth).abs() < 0.003, "{at}: {placed} vs {truth}");
    }
}

#[test]
fn a_stalled_or_broken_anchor_is_skipped_not_trusted() {
    // A channel that stopped repeats its frame count at a later host time,
    // and an absent one carries host 0. Neither may bend the line: frame
    // 160000 of a nominal channel is 10 s in, whatever those anchors say.
    let mut stalled = segment(0, START_NS, 20.0, Clock { ppm: 0.0 }, Clock { ppm: 0.0 });
    stalled.anchors = vec![
        Anchor {
            mic_host_ns: START_NS + 5_000_000_000,
            mic_frames: 80_000,
            sys_host_ns: 0,
            sys_frames: 80_000,
        },
        // Frozen frames: the stall.
        Anchor {
            mic_host_ns: START_NS + 9_000_000_000,
            mic_frames: 80_000,
            sys_host_ns: 0,
            sys_frames: 80_000,
        },
        // A slope ten times off nominal: a broken latch.
        Anchor {
            mic_host_ns: START_NS + 60_000_000_000,
            mic_frames: 100_000,
            sys_host_ns: 0,
            sys_frames: 100_000,
        },
        Anchor {
            mic_host_ns: START_NS + 15_000_000_000,
            mic_frames: 240_000,
            sys_host_ns: 0,
            sys_frames: 240_000,
        },
    ];
    let segments = recording(vec![stalled]);
    for channel in [Channel::Mic, Channel::System] {
        let placed = segments.frame_to_sec(channel, 160_000).unwrap();
        assert!((placed - 10.0).abs() < 1e-9, "{channel:?}: {placed}");
    }
}

#[test]
fn a_fractional_wav_second_and_its_frame_place_the_same() {
    // The engines report where a line starts in WAV seconds; the frame it
    // stands for must land on the same instant.
    let segments = recording(vec![segment(
        0,
        START_NS,
        600.0,
        Clock { ppm: 80.0 },
        Clock { ppm: -80.0 },
    )]);
    for frame in [0u64, 12_345, 4_000_000, 9_000_000] {
        let wav_sec = frame as f64 / 16_000.0;
        let by_frame = segments.frame_to_sec(Channel::Mic, frame).unwrap();
        let by_sec = segments.wav_sec_to_sec(Channel::Mic, wav_sec).unwrap();
        assert!((by_frame - by_sec).abs() < 1e-9, "{frame}");
    }
}
