use super::*;
use crate::session::{CollectingListener, SharedCollector};
use crate::vad::FRAME_SAMPLES;

/// A tone loud enough for the real detector, at 16 kHz.
fn speech(seconds: f64) -> Vec<i16> {
    let total = (seconds * SAMPLE_RATE as f64) as usize;
    (0..total)
        .map(|index| {
            let t = index as f64 / SAMPLE_RATE as f64;
            // Two formant-ish tones plus a syllable envelope. Not speech,
            // but nothing in this module claims to recognize it — it only
            // has to read as voiced to `earshot`.
            let carrier = (2.0 * std::f64::consts::PI * 140.0 * t).sin() * 0.6
                + (2.0 * std::f64::consts::PI * 700.0 * t).sin() * 0.3;
            let envelope = 0.5 + 0.5 * (2.0 * std::f64::consts::PI * 4.0 * t).sin();
            (carrier * envelope * 12_000.0) as i16
        })
        .collect()
}

fn silence(seconds: f64) -> Vec<i16> {
    vec![0; (seconds * SAMPLE_RATE as f64) as usize]
}

fn session(
    engine: &mut ReplayEngine,
    sink: SharedCollector,
    listener: CollectingListener,
) -> Box<dyn SttSession> {
    engine
        .start_session(
            SessionOptions::new(Speaker::You).with_volatile_per_sec(f64::INFINITY),
            Box::new(sink),
            Box::new(listener),
        )
        .expect("the replay engine streams")
}

#[test]
fn thirty_seconds_of_silence_produces_no_lines_and_no_tail() {
    // The canonical bug of this role, in its streaming shape.
    let mut engine = ReplayEngine::new();
    let sink = SharedCollector::new();
    let seen = CollectingListener::new();
    let mut live = session(&mut engine, sink.clone(), seen.clone());

    replay_samples(&silence(30.0), live.as_mut(), &ReplayOptions::instant()).unwrap();
    let outcome = live.finish().unwrap();

    assert_eq!(outcome.finalized, 0, "silence must not settle anything");
    assert!(sink.is_empty(), "silence reached disk: {:?}", sink.lines());
    assert!(seen.finals().is_empty());
    assert!(
        seen.volatiles().is_empty(),
        "the detector opened no span, so there was nothing to guess about"
    );
    assert_eq!(seen.tail_for(Speaker::You), None, "no stale tail");
    assert!(!outcome.discarded_volatile);
    assert_eq!(outcome.audio_sec, 30);
}

#[test]
fn a_hypothesis_that_never_settles_is_dropped_not_promoted() {
    // Speech still going when the meeting ends, cut short enough that the
    // minimum-length rule throws the span away. The guess was on screen;
    // it must come off, and it must not reach disk.
    let short = SegmentConfig {
        min_speech_frames: 10_000,
        ..SegmentConfig::default()
    };
    let mut engine = ReplayEngine::new().with_segmentation(short);
    let sink = SharedCollector::new();
    let seen = CollectingListener::new();
    let mut live = session(&mut engine, sink.clone(), seen.clone());

    replay_samples(&speech(3.0), live.as_mut(), &ReplayOptions::instant()).unwrap();
    assert!(
        seen.tail_for(Speaker::You).is_some(),
        "a hypothesis should be showing mid-utterance"
    );

    let outcome = live.finish().unwrap();
    assert!(outcome.discarded_volatile, "there was a guess to discard");
    assert_eq!(outcome.finalized, 0);
    assert!(sink.is_empty(), "a guess reached disk: {:?}", sink.lines());
    assert_eq!(
        seen.tail_for(Speaker::You),
        None,
        "finish must clear the tail or it sits on screen forever"
    );
}

#[test]
fn speech_then_silence_settles_a_line_and_clears_the_tail() {
    let mut engine = ReplayEngine::new();
    let sink = SharedCollector::new();
    let seen = CollectingListener::new();
    let mut live = session(&mut engine, sink.clone(), seen.clone());

    let mut pcm = speech(2.0);
    pcm.extend(silence(2.0));
    replay_samples(&pcm, live.as_mut(), &ReplayOptions::instant()).unwrap();
    let outcome = live.finish().unwrap();

    assert_eq!(outcome.finalized, 1, "one utterance, one line");
    assert_eq!(sink.len(), 1);
    assert_eq!(sink.utterances()[0].text, DEMO_SCRIPT_YOU[0]);
    assert!(
        !seen.volatiles().is_empty(),
        "the pane should have seen the guess grow"
    );
    assert_eq!(seen.tail_for(Speaker::You), None, "the final clears it");
    assert!(!outcome.discarded_volatile);
}

#[test]
fn the_guess_is_always_a_prefix_of_the_line_it_settles_as() {
    // If these two could disagree, the pane would visibly rewrite itself
    // at every final, which is the thing the tail contract is for.
    let mut engine = ReplayEngine::new();
    let sink = SharedCollector::new();
    let seen = CollectingListener::new();
    let mut live = session(&mut engine, sink.clone(), seen.clone());

    let mut pcm = speech(2.5);
    pcm.extend(silence(2.0));
    replay_samples(&pcm, live.as_mut(), &ReplayOptions::instant()).unwrap();
    live.finish().unwrap();

    let settled = &sink.utterances()[0].text;
    for guess in seen.volatiles() {
        assert!(
            settled.starts_with(&guess.text),
            "{:?} is not a prefix of {settled:?}",
            guess.text
        );
    }
}

#[test]
fn the_buffer_does_not_grow_with_the_meeting() {
    // A four-hour meeting must not be four hours of RAM. Ten minutes of
    // silence is the cheapest way to assert the buffer is pruned.
    let mut assembler = SpanAssembler::with_default_vad(SegmentConfig::default());
    let block = silence(1.0);
    for _ in 0..600 {
        assert!(assembler.push(&block).is_empty());
    }
    assert_eq!(assembler.fed_sec(), 600);
    assert!(
        assembler.buffered_samples() <= block.len() + FRAME_SAMPLES,
        "held {} samples after 10 minutes",
        assembler.buffered_samples()
    );
}

#[test]
fn live_and_batch_produce_the_same_transcript() {
    // If these two disagreed, the live pane and the saved `transcript.md`
    // would disagree, and the user would watch a line change after the
    // meeting ended.
    let mut pcm = speech(2.0);
    pcm.extend(silence(1.5));
    pcm.extend(speech(2.0));
    pcm.extend(silence(1.5));

    let tmp = tempfile::tempdir().unwrap();
    let wav = tmp.path().join("mic.wav");
    write_wav(&wav, &pcm);

    let mut engine = ReplayEngine::new();
    let sink = SharedCollector::new();
    let mut live = session(&mut engine, sink.clone(), CollectingListener::new());
    replay_samples(&pcm, live.as_mut(), &ReplayOptions::instant()).unwrap();
    live.finish().unwrap();

    let mut batch = crate::sink::CollectingSink::new();
    engine.transcribe(&wav, Speaker::You, &mut batch).unwrap();

    assert!(!batch.utterances.is_empty(), "the fixture has speech in it");
    assert_eq!(sink.lines(), batch.lines());
}

#[test]
fn a_track_replays_end_to_end_through_the_engine_trait() {
    // The shape Nia calls: nothing here names a concrete session type.
    let mut pcm = speech(2.0);
    pcm.extend(silence(1.5));
    let tmp = tempfile::tempdir().unwrap();
    let wav = tmp.path().join("mic.wav");
    write_wav(&wav, &pcm);

    let sink = SharedCollector::new();
    let seen = CollectingListener::new();
    let mut engine: Box<dyn SttEngine> = Box::new(ReplayEngine::new());
    let outcome = replay_track(
        &wav,
        engine.as_mut(),
        SessionOptions::new(Speaker::Others),
        Box::new(sink.clone()),
        Box::new(seen.clone()),
        &ReplayOptions::instant(),
    )
    .unwrap();

    assert_eq!(outcome.speaker, Speaker::Others);
    assert_eq!(outcome.engine, ReplayEngine::NAME);
    assert_eq!(outcome.finalized, 1);
    assert_eq!(sink.utterances()[0].text, DEMO_SCRIPT_OTHERS[0]);
    assert_eq!(seen.finals()[0].speaker, Speaker::Others);
}

#[test]
fn a_non_streaming_engine_says_so_instead_of_pretending() {
    struct BatchOnly;
    impl SttEngine for BatchOnly {
        fn name(&self) -> &'static str {
            "batch-only"
        }
        fn transcribe(
            &mut self,
            _wav: &Path,
            _speaker: Speaker,
            _sink: &mut dyn TranscriptSink,
        ) -> Result<(), Error> {
            Ok(())
        }
    }

    let mut engine = BatchOnly;
    assert!(!engine.supports_streaming());
    // `Box<dyn SttSession>` is not `Debug`, so match rather than unwrap.
    match engine.start_session(
        SessionOptions::new(Speaker::You),
        Box::new(SharedCollector::new()),
        Box::new(NoListener),
    ) {
        Err(Error::StreamingUnsupported(name)) => assert_eq!(name, "batch-only"),
        Err(other) => panic!("expected StreamingUnsupported, got {other:?}"),
        Ok(_) => panic!("an engine with no streaming path must not open a session"),
    }
}

fn write_wav(path: &Path, pcm: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for sample in pcm {
        writer.write_sample(*sample).unwrap();
    }
    writer.finalize().unwrap();
}
