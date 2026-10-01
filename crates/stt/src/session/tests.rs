use super::*;
use crate::vad::{FRAME_SAMPLES, SAMPLE_RATE, SegmentConfig, Vad};

/// No rate cap at all: `1.0 / INFINITY` is a zero minimum interval, so
/// every hypothesis is delivered. Tests that are about the *tail* rule use
/// this so they cannot fail on how fast the machine ran them; the two
/// tests that are about the cap itself set a real rate.
const UNCAPPED: f64 = f64::INFINITY;

fn emitter(listener: CollectingListener, rate: f64) -> LiveEmitter {
    let options = SessionOptions::new(Speaker::You).with_volatile_per_sec(rate);
    LiveEmitter::new(&options, Box::new(listener))
}

#[test]
fn a_volatile_never_reaches_the_sink() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), UNCAPPED);
    let sink = SharedCollector::new();

    emitter.volatile(1.0, "sessions are");
    emitter.volatile(1.0, "sessions are still");

    assert!(sink.is_empty(), "a volatile must never be persisted");
    assert_eq!(seen.volatiles().len(), 2);
}

#[test]
fn a_final_clears_the_tail_and_is_written_once() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), UNCAPPED);
    let mut sink = SharedCollector::new();

    emitter.volatile(1.0, "sessions are still");
    emitter
        .finalize(1.0, "Sessions are still in memory.", &mut sink)
        .unwrap();

    assert_eq!(sink.len(), 1);
    assert_eq!(
        sink.lines(),
        ["[00:00:01] You: Sessions are still in memory."]
    );
    assert_eq!(seen.tail_for(Speaker::You), None, "a final clears the tail");
    assert!(!emitter.has_tail());
}

#[test]
fn withdrawing_clears_a_tail_that_was_shown() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), UNCAPPED);

    emitter.volatile(1.0, "sessions are");
    assert_eq!(seen.tail_for(Speaker::You).as_deref(), Some("sessions are"));

    assert!(emitter.withdraw(), "there was a tail to withdraw");
    assert_eq!(
        seen.tail_for(Speaker::You),
        None,
        "a withdrawn hypothesis must not stay on screen"
    );
    assert!(matches!(
        seen.updates().last(),
        Some(LiveUpdate::Dropped { .. })
    ));
}

#[test]
fn withdrawing_nothing_says_nothing() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), UNCAPPED);
    assert!(!emitter.withdraw());
    assert!(seen.updates().is_empty(), "no tail, no Dropped update");
}

#[test]
fn the_rate_cap_coalesces_instead_of_queueing() {
    let seen = CollectingListener::new();
    // 1/sec: the first goes straight out, the rest are held.
    let mut emitter = emitter(seen.clone(), 1.0);

    for text in ["s", "se", "ses", "sess", "sessions"] {
        emitter.volatile(1.0, text);
    }

    let volatiles = seen.volatiles();
    assert_eq!(volatiles.len(), 1, "four updates were coalesced away");
    assert_eq!(volatiles[0].text, "s");
    // The newest is pending, not lost: a later poll delivers *it*, not the
    // stale ones in between.
    assert!(emitter.has_tail());
}

#[test]
fn a_coalesced_hypothesis_is_the_newest_one_not_the_oldest() {
    let seen = CollectingListener::new();
    let mut options = SessionOptions::new(Speaker::You);
    options.volatile_per_sec = 1000.0;
    let mut emitter = LiveEmitter::new(&options, Box::new(seen.clone()));

    // Force the cap on after the first delivery.
    emitter.volatile(1.0, "first");
    emitter.min_interval = Duration::from_secs(3600);
    emitter.volatile(1.0, "second");
    emitter.volatile(1.0, "third");
    // Lift it and poll, the way a session does on every feed.
    emitter.min_interval = Duration::ZERO;
    emitter.poll();

    let texts: Vec<_> = seen.volatiles().into_iter().map(|l| l.text).collect();
    assert_eq!(texts, ["first", "third"], "the superseded guess is dropped");
}

#[test]
fn an_unchanged_hypothesis_is_not_an_update() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), UNCAPPED);

    emitter.volatile(1.0, "sessions are");
    emitter.volatile(1.0, "sessions   are");
    emitter.volatile(1.0, "sessions are\n");

    assert_eq!(
        seen.volatiles().len(),
        1,
        "whitespace-only differences are the same hypothesis"
    );
}

#[test]
fn empty_text_is_never_a_line_and_never_a_tail() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), UNCAPPED);
    let mut sink = SharedCollector::new();

    emitter.volatile(1.0, "something");
    emitter.volatile(1.0, "   \n ");
    assert_eq!(
        seen.tail_for(Speaker::You),
        None,
        "blank withdraws the tail"
    );

    assert!(!emitter.finalize(1.0, "  ", &mut sink).unwrap());
    assert!(sink.is_empty());
}

#[test]
fn seq_is_meeting_global_across_both_speakers() {
    let seen = CollectingListener::new();
    let (mine, theirs) = SessionOptions::pair();
    let mut mic = LiveEmitter::new(&mine, Box::new(seen.clone()));
    let mut system = LiveEmitter::new(&theirs, Box::new(seen.clone()));
    let mut sink = SharedCollector::new();

    mic.finalize(1.0, "Morning.", &mut sink).unwrap();
    system
        .finalize(2.0, "Morning everyone.", &mut sink)
        .unwrap();
    mic.finalize(3.0, "Shall we start?", &mut sink).unwrap();

    let seqs: Vec<_> = seen.finals().into_iter().map(|line| line.seq).collect();
    assert_eq!(seqs, [0, 1, 2], "two tracks, one sequence, no collisions");
}

#[test]
fn a_rate_of_zero_turns_the_tail_off_entirely() {
    let seen = CollectingListener::new();
    let mut emitter = emitter(seen.clone(), 0.0);
    emitter.volatile(1.0, "not wanted");
    assert!(seen.updates().is_empty());
    assert!(!emitter.has_tail());
}

#[test]
fn the_live_line_serializes_with_its_kind() {
    let json = serde_json::to_string(&LiveUpdate::Volatile(LiveLine {
        seq: 7,
        speaker: Speaker::Others,
        start_sec: 12.5,
        text: "and the API".into(),
    }))
    .unwrap();
    assert!(json.contains("\"kind\":\"volatile\""), "{json}");
    assert!(json.contains("\"speaker\":\"others\""), "{json}");
    assert!(json.contains("\"seq\":7"), "{json}");
}

#[test]
fn a_dropped_update_serializes_for_the_pane() {
    let json = serde_json::to_string(&LiveUpdate::Dropped {
        speaker: Speaker::You,
        seq: 3,
    })
    .unwrap();
    assert_eq!(json, r#"{"kind":"dropped","speaker":"you","seq":3}"#);
}

struct Scripted {
    scores: Vec<f32>,
    next: usize,
}

impl Vad for Scripted {
    fn score(&mut self, _frame: &[i16]) -> f32 {
        let score = self.scores.get(self.next).copied().unwrap_or(0.0);
        self.next += 1;
        score
    }

    fn reset(&mut self) {
        self.next = 0;
    }
}

fn assembler_config() -> SegmentConfig {
    SegmentConfig {
        threshold: 0.5,
        onset_frames: 2,
        hangover_frames: 3,
        pad_frames: 2,
        min_speech_frames: 2,
        max_speech_frames: 1_000,
    }
}

/// 40 frames, speech on 4..=9 and 20..=25. The ramp makes every sample
/// unique, so a span's content can be checked against its position.
fn scripted_assembler() -> (SpanAssembler, Vec<i16>) {
    let mut scores = vec![0.0; 40];
    for frame in (4..=9).chain(20..=25) {
        scores[frame] = 0.9;
    }
    let vad = Box::new(Scripted { scores, next: 0 });
    let pcm = (0..40 * FRAME_SAMPLES)
        .map(|i| (i % 30_000) as i16)
        .collect();
    (SpanAssembler::new(assembler_config(), vad), pcm)
}

#[test]
fn the_assembler_hands_out_pinned_spans_whatever_the_block_size() {
    // (start_sample, len) of each span, pinned from the current output.
    // Small blocks lose one frame of lead padding: the buffer has already
    // trimmed it by the time the onset opens the span. Big blocks open the
    // span before any trim runs. Existing behaviour, pinned as it is.
    let small = [
        (3 * FRAME_SAMPLES, 9 * FRAME_SAMPLES),
        (19 * FRAME_SAMPLES, 9 * FRAME_SAMPLES),
    ];
    let big = [
        (2 * FRAME_SAMPLES, 10 * FRAME_SAMPLES),
        (18 * FRAME_SAMPLES, 10 * FRAME_SAMPLES),
    ];
    for (block, expected) in [
        (1, small),
        (97, small),
        (256, small),
        (300, small),
        (1_600, big),
        (40 * FRAME_SAMPLES, big),
    ] {
        let (mut assembler, pcm) = scripted_assembler();
        let mut got = Vec::new();
        for chunk in pcm.chunks(block) {
            got.extend(assembler.push(chunk));
        }
        got.extend(assembler.finish());

        let shape: Vec<_> = got
            .iter()
            .map(|s| ((s.start_sec * SAMPLE_RATE as f64) as usize, s.samples.len()))
            .collect();
        assert_eq!(shape, expected, "block size {block}");
        for (span, (start, len)) in got.iter().zip(expected) {
            assert_eq!(span.samples, pcm[start..start + len], "block size {block}");
        }
    }
}

#[test]
fn the_open_span_is_the_padded_audio_heard_so_far() {
    let (mut assembler, pcm) = scripted_assembler();
    assert!(assembler.push(&pcm[..8 * FRAME_SAMPLES]).is_empty());
    let open = assembler.open().expect("speech is open at frame 8");
    assert_eq!(
        open.start_sec,
        (2 * FRAME_SAMPLES) as f64 / SAMPLE_RATE as f64
    );
    assert_eq!(open.samples, pcm[2 * FRAME_SAMPLES..8 * FRAME_SAMPLES]);
}

#[test]
fn the_open_view_matches_the_owned_open_span() {
    let (mut assembler, pcm) = scripted_assembler();
    assembler.push(&pcm[..8 * FRAME_SAMPLES]);
    let owned = assembler.open().expect("open");
    let (start_sec, samples) = assembler.open_view().expect("open");
    assert_eq!(start_sec, owned.start_sec);
    assert_eq!(samples, owned.samples);
}

#[test]
fn the_assembler_buffer_capacity_settles_instead_of_growing() {
    let mut assembler = SpanAssembler::new(
        assembler_config(),
        Box::new(Scripted {
            scores: vec![],
            next: 0,
        }),
    );
    let block = vec![0i16; 1_600];
    for _ in 0..50 {
        assembler.push(&block);
    }
    let settled = assembler.buffer_capacity();
    for _ in 0..5_000 {
        assembler.push(&block);
    }
    assert_eq!(assembler.buffer_capacity(), settled);
    assert!(assembler.buffered_samples() < 4 * FRAME_SAMPLES + 1_600);
}
