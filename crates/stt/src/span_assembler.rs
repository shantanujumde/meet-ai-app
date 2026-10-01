//! Cuts speech spans out of a held PCM stream for the live session.

use crate::vad::{
    EarshotVad, FRAME_SAMPLES, SAMPLE_RATE, SegmentConfig, Segmenter, SpeechSpan, Vad, pad_span,
};

/// A chunk of audio the VAD has decided is worth transcribing.
///
/// Already padded, already cut out of the stream, and positioned on the
/// recording's timeline rather than the chunk's.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadySpan {
    /// Seconds from the start of the recording to the first sample.
    pub start_sec: f64,
    pub samples: Vec<i16>,
}

impl ReadySpan {
    pub fn duration_sec(&self) -> f64 {
        self.samples.len() as f64 / SAMPLE_RATE as f64
    }
}

/// Turns a live sample stream into the spans an engine is allowed to see.
///
/// This is [`crate::vad::detect_speech`] for audio that has not finished
/// arriving: the same [`Segmenter`], the same [`SegmentConfig`], the same
/// hallucination guard — silence yields no spans, so a live engine is no more
/// able to invent a line than a batch one is. What it adds is a buffer that
/// remembers only as much audio as a span could still need, so a four-hour
/// meeting does not sit in RAM.
///
/// Every streaming engine drives one. Sharing it is what makes the silence
/// gate a property of the crate rather than of each engine's own care.
pub struct SpanAssembler {
    config: SegmentConfig,
    vad: Box<dyn Vad>,
    segmenter: Segmenter,
    /// Audio still in reach of a span. `start` is `buffer[0]`'s absolute
    /// sample index in the recording.
    buffer: Vec<i16>,
    start: usize,
    /// Absolute end of the last span handed out. Padding may not reach back
    /// past it: that audio is already in a finalized line, and repeating it
    /// would repeat the words.
    released: usize,
    /// Absolute count of samples fed so far.
    fed: usize,
}

impl SpanAssembler {
    pub fn new(config: SegmentConfig, vad: Box<dyn Vad>) -> Self {
        Self {
            config,
            vad,
            segmenter: Segmenter::new(config),
            // Two seconds up front: the buffer then only grows for a long
            // utterance, not on the first few blocks.
            buffer: Vec::with_capacity(2 * SAMPLE_RATE as usize),
            start: 0,
            released: 0,
            fed: 0,
        }
    }

    /// The default detector, which is what every engine actually uses.
    pub fn with_default_vad(config: SegmentConfig) -> Self {
        Self::new(config, Box::new(EarshotVad::new()))
    }

    /// Add the next block of 16 kHz mono PCM; get back any spans it settled.
    ///
    /// Usually empty — a span only settles when the speaker stops, or when a
    /// monologue hits [`SegmentConfig::max_speech_frames`].
    pub fn push(&mut self, samples: &[i16]) -> Vec<ReadySpan> {
        self.buffer.extend_from_slice(samples);
        self.fed += samples.len();

        let mut ready = Vec::new();
        loop {
            let frame_start = self.segmenter.frames_scored() * FRAME_SAMPLES;
            // Frames are scored in stream order and never re-scored, so the
            // next one is always at or after the front of the buffer.
            let Some(offset) = frame_start.checked_sub(self.start) else {
                break;
            };
            if offset + FRAME_SAMPLES > self.buffer.len() {
                break;
            }

            let frame = &self.buffer[offset..offset + FRAME_SAMPLES];
            let is_speech = self.vad.score(frame) >= self.config.threshold;
            if let Some(raw) = self.segmenter.push(is_speech) {
                ready.push(self.release(raw));
            }
        }

        self.trim();
        ready
    }

    /// The utterance in progress, if someone is mid-sentence.
    ///
    /// This is what a volatile hypothesis is made of. It is *not* released:
    /// the same audio comes back in the settled span later, because a
    /// hypothesis is a guess and the final pass has to see the whole thing.
    pub fn open(&self) -> Option<ReadySpan> {
        let (start_sec, samples) = self.open_view()?;
        Some(ReadySpan {
            start_sec,
            samples: samples.to_vec(),
        })
    }

    /// [`Self::open`] without the copy: the start and a slice into the held
    /// audio. The per-block path uses this, since a guess runs on every block
    /// and the open span can be 25 s long.
    pub fn open_view(&self) -> Option<(f64, &[i16])> {
        let raw = self.segmenter.open_span()?;
        let span = pad_span(raw, &self.config, self.released, self.fed);
        let (start_sec, from, to) = self.locate(span);
        Some((start_sec, &self.buffer[from..to]))
    }

    #[cfg(test)]
    pub(crate) fn buffer_capacity(&self) -> usize {
        self.buffer.capacity()
    }

    pub fn has_open(&self) -> bool {
        self.segmenter.open_span().is_some()
    }

    /// End of stream: settle whatever was still open.
    pub fn finish(&mut self) -> Option<ReadySpan> {
        let raw = self.segmenter.finish()?;
        Some(self.release(raw))
    }

    /// Whole seconds of audio fed in.
    pub fn fed_sec(&self) -> u64 {
        self.fed as u64 / SAMPLE_RATE as u64
    }

    /// How much audio is being held. The live-memory assertion reads this.
    pub fn buffered_samples(&self) -> usize {
        self.buffer.len()
    }

    /// Pad a raw span, mark its audio spent, and cut it out.
    fn release(&mut self, raw: SpeechSpan) -> ReadySpan {
        let floor = self.released.max(self.start);
        let span = pad_span(raw, &self.config, floor, self.fed);
        self.released = span.end_sample;
        self.cut(span)
    }

    /// Start in seconds and the buffer range `span` covers.
    fn locate(&self, span: SpeechSpan) -> (f64, usize, usize) {
        let from = span
            .start_sample
            .saturating_sub(self.start)
            .min(self.buffer.len());
        let to = span
            .end_sample
            .saturating_sub(self.start)
            .clamp(from, self.buffer.len());
        (span.start_sample as f64 / SAMPLE_RATE as f64, from, to)
    }

    fn cut(&self, span: SpeechSpan) -> ReadySpan {
        let (start_sec, from, to) = self.locate(span);
        ReadySpan {
            start_sec,
            samples: self.buffer[from..to].to_vec(),
        }
    }

    /// Drop audio no span can reach any more.
    ///
    /// The earliest sample still in play is the open span's start, or — if
    /// nothing is open — the next frame to be scored, since a span could open
    /// there. Either way it is the context pad that decides how far back the
    /// engine may still look.
    fn trim(&mut self) {
        let pad = self.config.pad_frames * FRAME_SAMPLES;
        let earliest = match self.segmenter.open_span() {
            Some(open) => open.start_sample,
            None => self.segmenter.frames_scored() * FRAME_SAMPLES,
        }
        .saturating_sub(pad);

        let keep_from = earliest.clamp(self.start, self.start + self.buffer.len());
        let drop = keep_from - self.start;
        if drop > 0 {
            self.buffer.drain(..drop);
            self.start = keep_from;
        }
    }
}
