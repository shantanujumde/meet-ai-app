//! Live speech timeline for engines that pick their own utterance boundaries.

use super::segmenter::{Segmenter, pad_span, push_span};
use super::{EarshotVad, FRAME_SAMPLES, SAMPLE_RATE, SegmentConfig, SpeechSpan, Vad};

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
            // Taken out so `score` can borrow `self`; put back with its
            // capacity, so the carry never reallocates.
            let mut carry = std::mem::take(&mut self.carry);
            self.score(&carry);
            carry.clear();
            self.carry = carry;
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

    /// Capacity of the carry buffer, for the no-reallocation test.
    #[cfg(test)]
    pub(super) fn carry_capacity(&self) -> usize {
        self.carry.capacity()
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
