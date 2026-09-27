//! Voice activity detection and utterance segmentation.
//!
//! This module is the first and most important half of the whisper
//! hallucination guard (SPEC §6). Whisper was trained on 30-second clips and,
//! handed a quiet stretch, will confidently invent subtitle-like text —
//! "Thank you.", "Thanks for watching!" — because that is what filled the quiet
//! parts of its training data. The fix is not to filter its output after the
//! fact. The fix is to never call it on audio that has no speech in it.
//!
//! So the rule for the whisper engine is absolute: **whisper only ever sees
//! sample ranges that [`detect_speech`] returned.** 30 seconds of silence
//! produces zero spans, therefore zero inference calls, therefore zero lines.
//!
//! [`Vad`] is a trait because SETUP.md §1.1 explicitly keeps the door open to
//! swapping `earshot` for Silero-via-`ort` if the WebRTC-style detector turns
//! out to be too weak on noisy input. Nothing outside this module names a
//! concrete detector.

/// Every engine in SPEC §2.5 takes 16 kHz mono, and `earshot` requires it.
pub const SAMPLE_RATE: u32 = 16_000;

/// `earshot` scores exactly 256 samples at a time — 16 ms at 16 kHz.
///
/// This is a property of the detector, not a tunable. It is public because the
/// span arithmetic below is easier to read in frames than in samples.
pub const FRAME_SAMPLES: usize = 256;

/// A voice activity detector over 16 kHz mono PCM.
pub trait Vad: Send {
    /// Score one [`FRAME_SAMPLES`]-long frame. Higher means more speech-like.
    ///
    /// Implementations are stateful across frames, which is why this takes
    /// `&mut self` and why frames must be fed in order.
    fn score(&mut self, frame: &[i16]) -> f32;

    /// Forget all history. Call between independent audio streams.
    fn reset(&mut self);
}

/// The v1 detector: `earshot` 1.2.2, pure Rust, `libm` its only dependency.
///
/// Note for anyone reading SETUP.md §1.1 alongside this: that section describes
/// `earshot` as a "WebRTC VAD port". At the pinned 1.2.2 it is not — it is a
/// small neural detector returning a 0..1 score per 16 ms frame. The choice
/// still holds (pure Rust, one dependency, no bundled ONNX model) but the
/// shape of the API is different from what the doc implies, so do not go
/// looking for WebRTC's aggressiveness modes. There are none; there is a
/// threshold, and it lives in [`SegmentConfig`].
pub struct EarshotVad {
    // Boxed on purpose: `earshot::Detector` puts ~8 KiB of state on the stack,
    // and `default_boxed` builds it on the heap without that detour.
    detector: Box<earshot::Detector>,
}

impl EarshotVad {
    pub fn new() -> Self {
        Self {
            detector: earshot::Detector::default_boxed(),
        }
    }
}

impl Default for EarshotVad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad for EarshotVad {
    fn score(&mut self, frame: &[i16]) -> f32 {
        // `predict_i16` returns -1.0 for a wrong-sized frame rather than
        // panicking in release. Treat that as "no speech" so a ragged tail
        // frame can never open a span.
        self.detector.predict_i16(frame)
    }

    fn reset(&mut self) {
        self.detector.reset();
    }
}

/// How aggressively speech is split into utterances.
///
/// The defaults are tuned for meeting audio: quick to open a span so no word is
/// clipped, slow to close one so a natural pause mid-sentence does not chop an
/// utterance in half.
#[derive(Debug, Clone, Copy)]
pub struct SegmentConfig {
    /// Frame score above which a frame counts as speech.
    ///
    /// `earshot` documents 0.5 as the generic threshold. We sit slightly above
    /// it because a false positive here costs a hallucinated transcript line,
    /// while a false negative costs a few clipped milliseconds that the
    /// [`Self::pad_frames`] context window gives back anyway.
    pub threshold: f32,

    /// Consecutive speech frames required to open an utterance (~onset).
    ///
    /// A single loud frame is a door slam, not a word.
    pub onset_frames: usize,

    /// Consecutive non-speech frames required to close an utterance.
    ///
    /// This is the "hangover". Too short and one sentence becomes four lines;
    /// too long and two speakers' turns merge.
    pub hangover_frames: usize,

    /// Context kept either side of a span before handing it to the engine.
    ///
    /// Whisper's first and last word are the ones it gets wrong when a clip
    /// starts mid-phoneme.
    pub pad_frames: usize,

    /// Spans shorter than this are dropped entirely.
    ///
    /// The second half of the hallucination guard: a 100 ms blip of keyboard
    /// noise that squeaks past the threshold still never reaches whisper.
    pub min_speech_frames: usize,
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            threshold: 0.6,
            onset_frames: 4,       // ~64 ms
            hangover_frames: 30,   // ~480 ms
            pad_frames: 10,        // ~160 ms either side
            min_speech_frames: 16, // ~256 ms
        }
    }
}

/// A contiguous stretch of audio believed to contain speech.
///
/// Sample indices into the 16 kHz mono buffer, half-open: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSpan {
    pub start_sample: usize,
    pub end_sample: usize,
}

impl SpeechSpan {
    /// Offset of this span from the start of the recording, in seconds.
    pub fn start_sec(self) -> f64 {
        self.start_sample as f64 / SAMPLE_RATE as f64
    }

    pub fn duration_sec(self) -> f64 {
        (self.end_sample - self.start_sample) as f64 / SAMPLE_RATE as f64
    }

    pub fn samples(self, pcm: &[i16]) -> &[i16] {
        &pcm[self.start_sample..self.end_sample.min(pcm.len())]
    }
}

/// Split 16 kHz mono PCM into the stretches that contain speech.
///
/// Returns an empty vector for silence. That empty vector is the whole
/// hallucination guard: the whisper engine iterates over the result, so nothing
/// to iterate means nothing to transcribe and nothing to write.
pub fn detect_speech(pcm: &[i16], vad: &mut dyn Vad, config: &SegmentConfig) -> Vec<SpeechSpan> {
    vad.reset();

    let total_frames = pcm.len() / FRAME_SAMPLES;
    let mut spans = Vec::new();

    // `None` = currently in silence. `Some(start)` = currently inside a span
    // that opened at frame `start`.
    let mut open_at: Option<usize> = None;
    let mut speech_run = 0usize;
    let mut silence_run = 0usize;
    // Where speech was last actually seen, so the hangover tail is trimmed back
    // off the end of the span rather than baked into it.
    let mut last_speech_frame = 0usize;

    for frame_index in 0..total_frames {
        let start = frame_index * FRAME_SAMPLES;
        let frame = &pcm[start..start + FRAME_SAMPLES];
        let is_speech = vad.score(frame) >= config.threshold;

        if is_speech {
            speech_run += 1;
            silence_run = 0;
            last_speech_frame = frame_index;
            if open_at.is_none() && speech_run >= config.onset_frames {
                // Open the span at the first frame of the run, not the frame
                // that crossed the onset count, or we clip the word's attack.
                open_at = Some(frame_index + 1 - speech_run);
            }
        } else {
            speech_run = 0;
            silence_run += 1;
            if let Some(start_frame) = open_at
                && silence_run >= config.hangover_frames
            {
                push_span(
                    &mut spans,
                    start_frame,
                    last_speech_frame + 1,
                    pcm.len(),
                    config,
                );
                open_at = None;
            }
        }
    }

    // A span still open at end-of-audio closes at the last speech frame.
    if let Some(start_frame) = open_at {
        push_span(
            &mut spans,
            start_frame,
            last_speech_frame + 1,
            pcm.len(),
            config,
        );
    }

    spans
}

/// Apply the minimum-length rule and the context padding, then record the span.
fn push_span(
    spans: &mut Vec<SpeechSpan>,
    start_frame: usize,
    end_frame: usize,
    pcm_len: usize,
    config: &SegmentConfig,
) {
    if end_frame.saturating_sub(start_frame) < config.min_speech_frames {
        return;
    }

    let padded_start = start_frame.saturating_sub(config.pad_frames);
    let padded_end = end_frame + config.pad_frames;

    let span = SpeechSpan {
        start_sample: padded_start * FRAME_SAMPLES,
        end_sample: (padded_end * FRAME_SAMPLES).min(pcm_len),
    };

    // Padding can make two close spans overlap. Merge rather than emit
    // overlapping audio, which would duplicate words across two lines.
    if let Some(previous) = spans.last_mut()
        && span.start_sample <= previous.end_sample
    {
        previous.end_sample = span.end_sample.max(previous.end_sample);
        return;
    }

    spans.push(span);
}

#[cfg(test)]
mod tests {
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
}
