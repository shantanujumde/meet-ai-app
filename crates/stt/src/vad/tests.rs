use super::*;

/// A stand-in detector driven by a script of scores, so the segmentation
/// state machine can be tested without depending on `earshot`'s judgement.
struct ScriptedVad {
    scores: Vec<f32>,
    next: usize,
}

impl Vad for ScriptedVad {
    fn score(&mut self, _frame: &[i16]) -> f32 {
        let score = self.scores.get(self.next).copied().unwrap_or(0.0);
        self.next += 1;
        score
    }

    fn reset(&mut self) {
        self.next = 0;
    }
}

fn config() -> SegmentConfig {
    SegmentConfig {
        threshold: 0.5,
        onset_frames: 2,
        hangover_frames: 3,
        pad_frames: 0,
        min_speech_frames: 2,
        max_speech_frames: 1_000,
    }
}

fn pcm_for(frames: usize) -> Vec<i16> {
    vec![0; frames * FRAME_SAMPLES]
}

#[test]
fn silence_produces_no_spans() {
    let scores = vec![0.0; 40];
    let mut vad = ScriptedVad { scores, next: 0 };
    let spans = detect_speech(&pcm_for(40), &mut vad, &config());
    assert!(spans.is_empty(), "silence must not open a span: {spans:?}");
}

#[test]
fn a_single_loud_frame_is_not_an_utterance() {
    // One frame over threshold, surrounded by silence. Below `onset_frames`,
    // so it never opens a span — this is the door-slam case.
    let mut scores = vec![0.0; 20];
    scores[10] = 0.9;
    let mut vad = ScriptedVad { scores, next: 0 };
    assert!(detect_speech(&pcm_for(20), &mut vad, &config()).is_empty());
}

#[test]
fn a_short_blip_is_dropped_by_the_minimum_length_rule() {
    // Long enough to open (2 frames) but shorter than `min_speech_frames`
    // is not reachable with these numbers, so raise the minimum for this
    // case specifically and confirm the rule bites.
    let mut scores = vec![0.0; 20];
    scores[5] = 0.9;
    scores[6] = 0.9;
    let mut vad = ScriptedVad { scores, next: 0 };
    let strict = SegmentConfig {
        min_speech_frames: 5,
        ..config()
    };
    assert!(detect_speech(&pcm_for(20), &mut vad, &strict).is_empty());
}

#[test]
fn a_speech_run_becomes_one_span_that_excludes_the_hangover() {
    // Frames 4..=9 are speech, everything else silence.
    let mut scores = vec![0.0; 24];
    for score in scores.iter_mut().take(10).skip(4) {
        *score = 0.9;
    }
    let mut vad = ScriptedVad { scores, next: 0 };
    let spans = detect_speech(&pcm_for(24), &mut vad, &config());

    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].start_sample, 4 * FRAME_SAMPLES);
    // Ends at the last speech frame + 1, NOT after the hangover — otherwise
    // every utterance would carry half a second of silence into whisper.
    assert_eq!(spans[0].end_sample, 10 * FRAME_SAMPLES);
}

#[test]
fn a_pause_shorter_than_the_hangover_keeps_one_utterance_together() {
    // Speech, a 2-frame pause (hangover is 3), then more speech.
    let mut scores = vec![0.0; 30];
    for index in [2, 3, 4, 5, 8, 9, 10, 11] {
        scores[index] = 0.9;
    }
    let mut vad = ScriptedVad { scores, next: 0 };
    let spans = detect_speech(&pcm_for(30), &mut vad, &config());

    assert_eq!(spans.len(), 1, "a mid-sentence pause must not split a line");
    assert_eq!(spans[0].start_sample, 2 * FRAME_SAMPLES);
    assert_eq!(spans[0].end_sample, 12 * FRAME_SAMPLES);
}

#[test]
fn a_pause_longer_than_the_hangover_splits_two_utterances() {
    let mut scores = vec![0.0; 40];
    for index in [2, 3, 4, 5, 20, 21, 22, 23] {
        scores[index] = 0.9;
    }
    let mut vad = ScriptedVad { scores, next: 0 };
    let spans = detect_speech(&pcm_for(40), &mut vad, &config());

    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].start_sample, 2 * FRAME_SAMPLES);
    assert_eq!(spans[1].start_sample, 20 * FRAME_SAMPLES);
}

#[test]
fn padding_merges_spans_instead_of_overlapping_them() {
    let mut scores = vec![0.0; 40];
    for index in [2, 3, 4, 5, 20, 21, 22, 23] {
        scores[index] = 0.9;
    }
    let mut vad = ScriptedVad { scores, next: 0 };
    // 10 frames of padding either side closes the 14-frame gap.
    let padded = SegmentConfig {
        pad_frames: 10,
        ..config()
    };
    let spans = detect_speech(&pcm_for(40), &mut vad, &padded);

    assert_eq!(spans.len(), 1, "overlapping padded spans must merge");
    for pair in spans.windows(2) {
        assert!(pair[0].end_sample <= pair[1].start_sample);
    }
}

// --- the streaming state machine ---
//
// The live path cannot call `detect_speech`, because it never has the
// whole recording. It drives `Segmenter` a frame at a time instead, so the
// two have to agree about where utterances start and stop.

fn drive(segmenter: &mut Segmenter, verdicts: &[bool]) -> Vec<SpeechSpan> {
    verdicts
        .iter()
        .filter_map(|is_speech| segmenter.push(*is_speech))
        .collect()
}

fn verdicts(total: usize, speech: &[usize]) -> Vec<bool> {
    (0..total).map(|index| speech.contains(&index)).collect()
}

#[test]
fn the_streaming_segmenter_agrees_with_the_batch_one() {
    let speech = [2, 3, 4, 5, 20, 21, 22, 23];
    let mut scores = vec![0.0; 40];
    for index in speech {
        scores[index] = 0.9;
    }
    let mut vad = ScriptedVad { scores, next: 0 };
    let batch = detect_speech(&pcm_for(40), &mut vad, &config());

    let mut segmenter = Segmenter::new(config());
    let mut live = drive(&mut segmenter, &verdicts(40, &speech));
    live.extend(segmenter.finish());

    // pad_frames is 0 in this config, so the padded batch spans and the
    // raw streaming ones are directly comparable.
    assert_eq!(batch, live);
}

#[test]
fn streaming_silence_never_opens_a_span() {
    let mut segmenter = Segmenter::new(config());
    assert!(drive(&mut segmenter, &[false; 200]).is_empty());
    assert_eq!(segmenter.open_span(), None, "nothing may be open");
    assert_eq!(segmenter.finish(), None);
}

#[test]
fn the_open_span_is_visible_before_it_closes() {
    // The live pane's volatile tail is drawn from this.
    let mut segmenter = Segmenter::new(config());
    drive(&mut segmenter, &verdicts(6, &[2, 3, 4, 5]));

    let open = segmenter.open_span().expect("speech is still going");
    assert_eq!(open.start_sample, 2 * FRAME_SAMPLES);
    assert_eq!(open.end_sample, 6 * FRAME_SAMPLES);

    // Three silent frames (the hangover) settle it.
    let closed = drive(&mut segmenter, &[false, false, false]);
    assert_eq!(closed.len(), 1);
    assert_eq!(segmenter.open_span(), None, "the tail must be released");
}

#[test]
fn a_blip_too_short_to_keep_still_closes_the_open_span() {
    // A span that opens and is then thrown away by the minimum-length rule
    // returns nothing from `push`. The live session has to notice that and
    // clear the tail, so `open_span` going back to `None` is the signal it
    // watches — assert that it actually does.
    let strict = SegmentConfig {
        min_speech_frames: 5,
        ..config()
    };
    let mut segmenter = Segmenter::new(strict);
    // Four frames only: two of silence, then two of speech. Any more and
    // the hangover would already have closed the span before the assert.
    drive(&mut segmenter, &verdicts(4, &[2, 3]));
    assert!(segmenter.open_span().is_some());

    let closed = drive(&mut segmenter, &[false, false, false]);
    assert!(closed.is_empty(), "too short to transcribe");
    assert_eq!(segmenter.open_span(), None, "but definitely not still open");
}

#[test]
fn a_monologue_is_cut_at_the_ceiling_rather_than_silently_truncated() {
    let capped = SegmentConfig {
        max_speech_frames: 10,
        ..config()
    };
    let mut segmenter = Segmenter::new(capped);
    // 25 unbroken frames of speech: two full cuts, third still open.
    let spans = drive(&mut segmenter, &[true; 25]);

    assert_eq!(spans.len(), 2, "cut twice: {spans:?}");
    assert_eq!(spans[0].start_sample, 0);
    assert_eq!(spans[0].end_sample, 10 * FRAME_SAMPLES);
    assert_eq!(spans[1].start_sample, 10 * FRAME_SAMPLES);
    assert_eq!(spans[1].end_sample, 20 * FRAME_SAMPLES);
    // No audio is lost at a cut: the next span starts where the last ended.
    let open = segmenter.open_span().expect("still talking");
    assert_eq!(open.start_sample, 20 * FRAME_SAMPLES);
}

#[test]
fn padding_never_reaches_back_into_audio_already_transcribed() {
    let span = SpeechSpan {
        start_sample: 10 * FRAME_SAMPLES,
        end_sample: 20 * FRAME_SAMPLES,
    };
    let padded = pad_span(span, &config(), 0, usize::MAX);
    assert_eq!(padded, span, "pad_frames is 0 here");

    let generous = SegmentConfig {
        pad_frames: 100,
        ..config()
    };
    let floor = 8 * FRAME_SAMPLES;
    let ceiling = 21 * FRAME_SAMPLES;
    let padded = pad_span(span, &generous, floor, ceiling);
    assert_eq!(padded.start_sample, floor, "clamped to the floor");
    assert_eq!(padded.end_sample, ceiling, "clamped to what has arrived");
}

// --- the gate for engines that segment audio themselves ---
//
// `SpeechTimeline` is how an Apple result gets checked against the same
// detector whisper is gated by. These tests pin the gate without the
// sidecar: a synthetic result over a silent stretch must not count as heard.

fn timeline(scores: Vec<f32>, config: SegmentConfig) -> SpeechTimeline {
    SpeechTimeline::new(config, Box::new(ScriptedVad { scores, next: 0 }))
}

fn sec(frames: usize) -> f64 {
    (frames * FRAME_SAMPLES) as f64 / SAMPLE_RATE as f64
}

#[test]
fn a_result_over_a_silent_stretch_is_not_heard_speech() {
    // The room-tone failure, in miniature: the whole recording scores as
    // non-speech, and an engine claims a line over a long stretch of it.
    let mut heard = timeline(vec![0.0; 600], config());
    heard.push(&pcm_for(600));
    heard.finish();

    assert!(heard.spans().is_empty());
    assert!(
        !heard.heard_speech(0.0, sec(540)),
        "a line settled over nothing but quiet must not count as speech"
    );
    assert!(
        !heard.heard_speech(sec(100), sec(100)),
        "nor an instant of it"
    );
}

#[test]
fn a_result_overlapping_speech_is_kept_even_when_it_overruns_the_span() {
    // Speech in frames 40..60 only. Apple's ranges for a real line often
    // start before the detector's span and end after it, so overlap, not
    // containment, is what keeps a real line.
    let mut scores = vec![0.0; 200];
    for score in scores.iter_mut().take(60).skip(40) {
        *score = 0.9;
    }
    let mut heard = timeline(scores, config());
    heard.push(&pcm_for(200));
    heard.finish();

    assert!(heard.heard_speech(sec(30), sec(45)), "overlaps the start");
    assert!(heard.heard_speech(sec(55), sec(90)), "overlaps the end");
    assert!(heard.heard_speech(sec(0), sec(200)), "covers it entirely");
    assert!(heard.heard_speech(sec(50), sec(50)), "an instant inside it");
    assert!(!heard.heard_speech(sec(0), sec(39)), "entirely before it");
    assert!(!heard.heard_speech(sec(61), sec(200)), "entirely after it");
}

#[test]
fn the_padding_whisper_gets_is_the_padding_the_gate_allows() {
    let mut scores = vec![0.0; 200];
    for score in scores.iter_mut().take(60).skip(40) {
        *score = 0.9;
    }
    let padded = SegmentConfig {
        pad_frames: 5,
        ..config()
    };
    let mut heard = timeline(scores, padded);
    heard.push(&pcm_for(200));
    heard.finish();

    // The span is frames 40..60, padded to 35..65.
    assert!(heard.heard_speech(sec(30), sec(36)), "reaches into the pad");
    assert!(heard.heard_speech(sec(64), sec(90)), "reaches into the pad");
    assert!(!heard.heard_speech(sec(30), sec(34)), "stops short of it");
    assert!(!heard.heard_speech(sec(66), sec(90)), "starts past it");
}

#[test]
fn the_timeline_settles_the_same_spans_detect_speech_returns() {
    // One detector, one answer. If these drifted, Apple would keep lines
    // over audio that whisper was never allowed to hear, or the reverse.
    let mut scores = vec![0.0; 120];
    for index in [2, 3, 4, 5, 20, 21, 22, 23, 24, 60, 61, 62, 63, 64, 118, 119] {
        scores[index] = 0.9;
    }
    let padded = SegmentConfig {
        pad_frames: 8,
        ..config()
    };
    let pcm = pcm_for(120);

    let mut vad = ScriptedVad {
        scores: scores.clone(),
        next: 0,
    };
    let batch = detect_speech(&pcm, &mut vad, &padded);

    // Pushed in ragged blocks that do not divide a frame, the way the live
    // tap delivers audio.
    let mut heard = timeline(scores, padded);
    for block in pcm.chunks(1_000) {
        heard.push(block);
    }
    heard.finish();

    assert_eq!(heard.spans(), batch.as_slice());
}

#[test]
fn an_absurd_timestamp_is_answered_rather_than_panicking() {
    // The sidecar sanitizes NaN, but nothing stops a corrupt or huge
    // CMTime from arriving. The reader thread must answer, not panic.
    let mut scores = vec![0.0; 200];
    for score in scores.iter_mut().take(60).skip(40) {
        *score = 0.9;
    }
    let mut heard = timeline(scores, config());
    heard.push(&pcm_for(200));

    assert!(!heard.heard_speech(1e300, 1e300), "far past the end");
    assert!(!heard.heard_speech(f64::INFINITY, f64::INFINITY));
    assert!(!heard.heard_speech(f64::MAX, 0.0), "end before start");
    assert!(
        heard.heard_speech(0.0, 1e300),
        "a huge range still overlaps"
    );
    assert!(heard.heard_speech(-1e300, f64::INFINITY));
    assert!(!heard.heard_speech(f64::NAN, f64::NAN), "NaN reads as 0");
}

#[test]
fn speech_still_inside_the_hangover_already_counts() {
    // Live, a line can settle before the detector has closed its span.
    let mut scores = vec![0.0; 50];
    for score in scores.iter_mut().take(20).skip(10) {
        *score = 0.9;
    }
    let mut heard = timeline(scores, config());
    heard.push(&pcm_for(21));

    assert!(heard.spans().is_empty(), "nothing has settled yet");
    assert!(heard.heard_speech(sec(12), sec(18)));
    assert!(!heard.heard_speech(sec(0), sec(9)));
}

#[test]
fn earshot_finds_no_speech_in_digital_silence() {
    // The real detector, not the scripted one. 30 s of zeroes.
    let pcm = vec![0i16; SAMPLE_RATE as usize * 30];
    let mut vad = EarshotVad::new();
    let spans = detect_speech(&pcm, &mut vad, &SegmentConfig::default());
    assert!(
        spans.is_empty(),
        "earshot opened {} span(s) on pure silence",
        spans.len()
    );
}
