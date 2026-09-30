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
/// The rate `audio` writes, from the one definition in `meeting-format`.
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::SAMPLE_RATE;

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

    /// Hard ceiling on one span, after which it is cut and a new one opened.
    ///
    /// Whisper was trained on 30-second windows and silently truncates a
    /// longer clip, so an uninterrupted monologue has to be cut *somewhere*;
    /// cutting it deliberately at a known point beats discovering later that
    /// the tail of a four-minute answer was never transcribed. 25 s leaves
    /// room for the padding either side.
    pub max_speech_frames: usize,
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            threshold: 0.6,
            onset_frames: 4,          // ~64 ms
            hangover_frames: 30,      // ~480 ms
            pad_frames: 10,           // ~160 ms either side
            min_speech_frames: 16,    // ~256 ms
            max_speech_frames: 1_562, // ~25 s
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

/// The onset/hangover state machine, with no audio and no detector attached.
///
/// Split out of [`detect_speech`] so the live path can share it. A batch caller
/// has the whole recording and can merge two padded spans that overlap; a live
/// caller has already handed the earlier span to an engine and cannot take it
/// back. Both need the *same* answer to "where does this utterance start and
/// stop", which is exactly what this struct is, and nothing else.
///
/// Feed it one `is_speech` verdict per [`FRAME_SAMPLES`]-long frame, in order.
/// Spans come back **unpadded**: padding is a per-caller policy, because it is
/// the only part that differs between batch and live.
#[derive(Debug, Clone)]
pub struct Segmenter {
    config: SegmentConfig,
    /// Absolute index of the next frame to be scored. Absolute, not relative
    /// to the current `feed` call, so live timestamps land on the recording's
    /// timeline rather than on the chunk's.
    next_frame: usize,
    /// `None` = currently in silence. `Some(start)` = inside a span that opened
    /// at frame `start`.
    open_at: Option<usize>,
    speech_run: usize,
    silence_run: usize,
    /// Where speech was last actually seen, so the hangover tail is trimmed
    /// back off the end of the span rather than baked into it.
    last_speech_frame: usize,
}

impl Segmenter {
    pub fn new(config: SegmentConfig) -> Self {
        Self {
            config,
            next_frame: 0,
            open_at: None,
            speech_run: 0,
            silence_run: 0,
            last_speech_frame: 0,
        }
    }

    /// Score one frame's verdict. Returns a span if this frame closed one.
    pub fn push(&mut self, is_speech: bool) -> Option<SpeechSpan> {
        let frame_index = self.next_frame;
        self.next_frame += 1;

        if is_speech {
            self.speech_run += 1;
            self.silence_run = 0;
            self.last_speech_frame = frame_index;
            if self.open_at.is_none() && self.speech_run >= self.config.onset_frames {
                // Open the span at the first frame of the run, not the frame
                // that crossed the onset count, or we clip the word's attack.
                self.open_at = Some(frame_index + 1 - self.speech_run);
            }
            // Somebody who never pauses still has to be cut into clips an
            // engine can actually swallow. Close here and reopen immediately,
            // so the next frame continues the same monologue in a new span.
            if let Some(start_frame) = self.open_at
                && (frame_index + 1).saturating_sub(start_frame) >= self.config.max_speech_frames
            {
                self.open_at = Some(frame_index + 1);
                return self.raw_span(start_frame, frame_index + 1);
            }
            return None;
        }

        self.speech_run = 0;
        self.silence_run += 1;
        if let Some(start_frame) = self.open_at
            && self.silence_run >= self.config.hangover_frames
        {
            self.open_at = None;
            return self.raw_span(start_frame, self.last_speech_frame + 1);
        }
        None
    }

    /// The span that is open right now, if any — speech heard but not yet
    /// settled. The live path needs this to show a volatile tail; the batch
    /// path never asks.
    ///
    /// Unlike [`Self::push`], this ignores `min_speech_frames`: a hypothesis
    /// that is still growing has not had its chance to get long enough yet.
    pub fn open_span(&self) -> Option<SpeechSpan> {
        self.open_at.map(|start_frame| SpeechSpan {
            start_sample: start_frame * FRAME_SAMPLES,
            end_sample: (self.last_speech_frame + 1) * FRAME_SAMPLES,
        })
    }

    /// End of audio: close whatever is still open.
    pub fn finish(&mut self) -> Option<SpeechSpan> {
        let start_frame = self.open_at.take()?;
        self.raw_span(start_frame, self.last_speech_frame + 1)
    }

    /// How many frames have been scored. `frames_scored() * FRAME_SAMPLES` is
    /// the absolute sample position of the stream.
    pub fn frames_scored(&self) -> usize {
        self.next_frame
    }

    /// Apply the minimum-length rule — the second half of the hallucination
    /// guard — and convert frames to samples.
    fn raw_span(&self, start_frame: usize, end_frame: usize) -> Option<SpeechSpan> {
        if end_frame.saturating_sub(start_frame) < self.config.min_speech_frames {
            return None;
        }
        Some(SpeechSpan {
            start_sample: start_frame * FRAME_SAMPLES,
            end_sample: end_frame * FRAME_SAMPLES,
        })
    }
}

/// Widen a span by [`SegmentConfig::pad_frames`] either side, clamped.
///
/// Whisper's first and last word are the ones it gets wrong when a clip starts
/// mid-phoneme, so every span handed to an engine carries context. `floor` and
/// `ceiling` are the sample range the padded span may not escape: the live path
/// passes the end of the previous span and the end of the audio it holds, so
/// padding can never reach into audio that was already transcribed or audio
/// that has not arrived yet.
pub fn pad_span(
    span: SpeechSpan,
    config: &SegmentConfig,
    floor: usize,
    ceiling: usize,
) -> SpeechSpan {
    let pad = config.pad_frames * FRAME_SAMPLES;
    SpeechSpan {
        start_sample: span.start_sample.saturating_sub(pad).max(floor),
        end_sample: (span.end_sample + pad).min(ceiling),
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
    let mut segmenter = Segmenter::new(*config);
    let mut spans = Vec::new();

    for frame_index in 0..total_frames {
        let start = frame_index * FRAME_SAMPLES;
        let frame = &pcm[start..start + FRAME_SAMPLES];
        let is_speech = vad.score(frame) >= config.threshold;
        if let Some(span) = segmenter.push(is_speech) {
            push_span(&mut spans, span, pcm.len(), config);
        }
    }

    // A span still open at end-of-audio closes at the last speech frame.
    if let Some(span) = segmenter.finish() {
        push_span(&mut spans, span, pcm.len(), config);
    }

    spans
}

/// Pad a raw span and record it, merging it into the previous one if the
/// padding made the two overlap.
///
/// Merging is a batch-only luxury — nothing has been transcribed yet, so two
/// spans can still become one. Emitting overlapping audio instead would
/// duplicate words across two lines.
fn push_span(spans: &mut Vec<SpeechSpan>, raw: SpeechSpan, pcm_len: usize, config: &SegmentConfig) {
    let span = pad_span(raw, config, 0, pcm_len);

    if let Some(previous) = spans.last_mut()
        && span.start_sample <= previous.end_sample
    {
        previous.end_sample = span.end_sample.max(previous.end_sample);
        return;
    }

    spans.push(span);
}

/// Where the detector heard speech, for an engine that picks its own
/// utterance boundaries.
///
/// The whisper half of the silence gate works *before* inference: whisper
/// only ever sees the spans [`detect_speech`] returned. Apple's
/// `SpeechTranscriber` cannot be gated that way. SPEC §2.5 has it stream
/// long-form audio and decide where utterances start and stop, which is why
/// engine 1 needs no VAD chunking, so it hears the quiet stretches too. On
/// quiet pink noise it sometimes settles a line that is not there. The
/// measured case is `room-tone-30s.wav`: in about half of all pink-noise
/// draws, Apple finalizes `"I"` over the first few seconds. The fixture's
/// seed is one of those draws, so the gate is always exercised. Earshot
/// scores every frame of that noise no higher than digital zeroes.
///
/// So the Apple half applies the same judgement *after*. A result is kept only
/// if the audio range it claims overlaps a span this detector would have
/// handed whisper. It uses the same detector, the same [`SegmentConfig`], and
/// the same padding and minimum-length rules ([`detect_speech`] and this type
/// share [`Segmenter`] and `push_span`), so both engines agree on what counts
/// as speech. That is what SPEC §5's Phase 1 gate asks of *both* engines:
/// 30 seconds of silence produces zero transcript lines. The check is
/// *overlap*, not containment, on purpose. Apple's range for a real line
/// routinely runs past the VAD span at either end, and one word of heard
/// speech is enough to keep the whole line. Dropping real speech is a worse
/// bug than the one this fixes.
///
/// Feed it the stream in order with [`Self::push`]. What it keeps is spans,
/// not audio: one per utterance, so a four-hour meeting holds a few thousand
/// pairs of integers.
pub struct SpeechTimeline {
    config: SegmentConfig,
    vad: Box<dyn Vad>,
    segmenter: Segmenter,
    /// Padded, merged, in order: exactly what [`detect_speech`] would return
    /// for the audio pushed so far.
    spans: Vec<SpeechSpan>,
    /// The tail of the last push that did not fill a frame. The live tap's
    /// block sizes need not divide [`FRAME_SAMPLES`].
    carry: Vec<i16>,
    /// Absolute count of samples pushed so far.
    fed: usize,
}

impl SpeechTimeline {
    /// An empty timeline that will score audio with `vad` under `config`.
    ///
    /// Engines want [`Self::with_default_vad`]. Passing both explicitly is the
    /// seam for tests (a scripted detector) and for a future Silero swap
    /// (SETUP.md §1.1). Use a fresh detector per stream, because detectors
    /// carry state from one frame to the next.
    pub fn new(config: SegmentConfig, vad: Box<dyn Vad>) -> Self {
        Self {
            config,
            vad,
            segmenter: Segmenter::new(config),
            spans: Vec::new(),
            carry: Vec::with_capacity(FRAME_SAMPLES),
            fed: 0,
        }
    }

    /// The default detector and thresholds, which are what whisper uses.
    pub fn with_default_vad() -> Self {
        Self::new(SegmentConfig::default(), Box::new(EarshotVad::new()))
    }

    /// Score the next block of 16 kHz mono PCM, in stream order.
    pub fn push(&mut self, samples: &[i16]) {
        self.fed += samples.len();
        let mut rest = samples;

        if !self.carry.is_empty() {
            let take = (FRAME_SAMPLES - self.carry.len()).min(rest.len());
            self.carry.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.carry.len() < FRAME_SAMPLES {
                return;
            }
            let mut frame = [0i16; FRAME_SAMPLES];
            frame.copy_from_slice(&self.carry);
            self.carry.clear();
            self.score(&frame);
        }

        let (frames, remainder) = rest.as_chunks::<FRAME_SAMPLES>();
        for frame in frames {
            self.score(frame);
        }
        self.carry.extend_from_slice(remainder);
    }

    /// End of audio: settle whatever span is still open, under the same
    /// minimum-length rule [`detect_speech`] applies at end of recording.
    pub fn finish(&mut self) {
        if let Some(raw) = self.segmenter.finish() {
            push_span(&mut self.spans, raw, self.fed, &self.config);
        }
        // Now the end of the recording is known, padding may not run past
        // it, exactly as in `detect_speech`. Only the last span can.
        if let Some(last) = self.spans.last_mut() {
            last.end_sample = last.end_sample.min(self.fed);
        }
    }

    /// Did the detector hear speech anywhere in `[start_sec, end_sec]`?
    ///
    /// An engine result whose range is a single instant (`end_sec` at or
    /// before `start_sec`) is checked as that instant.
    ///
    /// A span that is still open counts even if it has not yet reached
    /// [`SegmentConfig::min_speech_frames`]. Live, Apple can settle a word
    /// while the speaker is still inside the hangover. Waiting to find out
    /// whether the span grows long enough is not an option, because the line
    /// has to be kept or dropped now and the file is append-only. Leaning
    /// towards keeping it costs, at worst, a line over a blip too short for
    /// whisper. The opposite choice would cost a real word.
    pub fn heard_speech(&self, start_sec: f64, end_sec: f64) -> bool {
        let rate = SAMPLE_RATE as f64;
        let start = (start_sec.max(0.0) * rate).floor() as usize;
        // Float-to-int casts saturate, so an absurd timestamp from the
        // sidecar lands at `usize::MAX` rather than wrapping. The `+ 1` has
        // to saturate too, or it would panic on the reader thread in debug.
        let end =
            ((end_sec.max(start_sec).max(0.0) * rate).ceil() as usize).max(start.saturating_add(1));
        let overlaps = |span: &SpeechSpan| span.start_sample < end && start < span.end_sample;

        // Starts and ends are both monotonic, so the first span that ends
        // after `start` is the only settled one that could overlap.
        let first = self.spans.partition_point(|span| span.end_sample <= start);
        if self.spans.get(first).is_some_and(overlaps) {
            return true;
        }
        self.segmenter
            .open_span()
            .map(|raw| pad_span(raw, &self.config, 0, usize::MAX))
            .is_some_and(|open| overlaps(&open))
    }

    /// Speech spans settled so far. Test affordance, and a way to see what
    /// the gate is deciding against.
    pub fn spans(&self) -> &[SpeechSpan] {
        &self.spans
    }

    fn score(&mut self, frame: &[i16]) {
        let is_speech = self.vad.score(frame) >= self.config.threshold;
        if let Some(raw) = self.segmenter.push(is_speech) {
            // No ceiling mid-stream. The audio the pad reaches for may simply
            // not have arrived yet, and cutting the pad off at the end of this
            // block would make the live answer depend on block sizes and
            // differ from the batch one. `finish` clips to the real end.
            push_span(&mut self.spans, raw, usize::MAX, &self.config);
        }
    }
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
}
