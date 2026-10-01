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

mod segmenter;
mod timeline;

pub use segmenter::{Segmenter, detect_speech, pad_span};
pub use timeline::SpeechTimeline;

#[cfg(test)]
mod tests;
