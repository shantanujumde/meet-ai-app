use super::*;
use crate::segments::{CHECKPOINT_INTERVAL_S, SCHEMA_VERSION, duration_ms, reason};

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
fn a_segment_missing_its_frame_counts_is_refused_not_zeroed() {
    // The shared schema would default these to 0 for stt's sake; the drift
    // path must refuse, as it did when the schema lived here.
    for missing in ["mic_frames", "sys_frames", "reason"] {
        let mut segment = serde_json::json!({
            "idx": 0, "start_host_ns": 0, "mic_rate": 16000, "sys_rate": 16000,
            "mic_frames": 16000, "sys_frames": 16000, "reason": "start",
        });
        segment.as_object_mut().unwrap().remove(missing);
        let json = serde_json::json!({ "version": 1, "segments": [segment] }).to_string();

        let err = Segments::from_json(&json).unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("missing field `{missing}`")),
            "{missing}: {err}"
        );
        // The tolerant shared parse still reads it — that is stt's path.
        assert!(serde_json::from_str::<Segments>(&json).is_ok());
    }
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
    let host_only_ms =
        (recording.segments[1].start_host_ns - recording.segments[0].start_host_ns) as f64 / 1e6
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
