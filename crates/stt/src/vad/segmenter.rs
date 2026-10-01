//! The onset/hangover state machine and batch segmentation.

use super::{FRAME_SAMPLES, SegmentConfig, SpeechSpan, Vad};

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
pub(super) fn push_span(
    spans: &mut Vec<SpeechSpan>,
    raw: SpeechSpan,
    pcm_len: usize,
    config: &SegmentConfig,
) {
    let span = pad_span(raw, config, 0, pcm_len);

    if let Some(previous) = spans.last_mut()
        && span.start_sample <= previous.end_sample
    {
        previous.end_sample = span.end_sample.max(previous.end_sample);
        return;
    }

    spans.push(span);
}
