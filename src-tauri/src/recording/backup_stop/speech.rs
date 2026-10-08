//! "Did someone speak?" for one channel of a recording (TUR-145).
//!
//! The silence stop reuses the voice detection the transcript already uses:
//! `stt::vad`'s detector (earshot) and its onset/hangover [`Segmenter`],
//! under the same [`SegmentConfig`] whisper's silence gate uses, so the stop
//! and the transcript agree on what counts as speech. It runs on its own copy
//! of each channel's 16 kHz frames, so it works whichever engine transcribes
//! and when nothing transcribes live (`transcription.live: false`).
//!
//! Speech counts once it is long enough not to be a door slam: a span the
//! segmenter settled (at least `min_speech_frames`, about a quarter of a
//! second), or one still open that is already that long.

use stt::vad::{EarshotVad, FRAME_SAMPLES, SegmentConfig, Segmenter, Vad};

/// One channel's speech detector.
pub struct SpeechGate {
    vad: Box<dyn Vad>,
    config: SegmentConfig,
    segmenter: Segmenter,
    /// The tail of the last block that did not fill a frame.
    carry: Vec<i16>,
}

impl SpeechGate {
    /// A gate scoring frames with `vad`. Tests pass a fake detector.
    pub fn new(vad: Box<dyn Vad>) -> Self {
        let config = SegmentConfig::default();
        Self {
            vad,
            config,
            segmenter: Segmenter::new(config),
            carry: Vec::with_capacity(FRAME_SAMPLES),
        }
    }

    /// The detector whisper's silence gate uses.
    pub fn earshot() -> Self {
        Self::new(Box::new(EarshotVad::new()))
    }

    /// Score the next block of the channel, in order. `true` when it held
    /// speech that counts.
    pub fn push(&mut self, samples: &[i16]) -> bool {
        let mut heard = false;
        let mut rest = samples;
        if !self.carry.is_empty() {
            let take = (FRAME_SAMPLES - self.carry.len()).min(rest.len());
            self.carry.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.carry.len() < FRAME_SAMPLES {
                return false;
            }
            // Taken out so `score` can borrow `self`; put back with its
            // capacity, so the carry never reallocates.
            let mut carry = std::mem::take(&mut self.carry);
            heard |= self.score(&carry);
            carry.clear();
            self.carry = carry;
        }
        let (frames, remainder) = rest.as_chunks::<FRAME_SAMPLES>();
        for frame in frames {
            heard |= self.score(frame);
        }
        self.carry.extend_from_slice(remainder);
        heard
    }

    fn score(&mut self, frame: &[i16]) -> bool {
        let is_speech = self.vad.score(frame) >= self.config.threshold;
        if self.segmenter.push(is_speech).is_some() {
            return true;
        }
        let long_enough = self.config.min_speech_frames * FRAME_SAMPLES;
        self.segmenter
            .open_span()
            .is_some_and(|open| open.end_sample - open.start_sample >= long_enough)
    }
}

/// A fake detector for tests: a frame is speech when it is loud, so a test
/// says "speech" or "silence" with the sample values alone.
#[cfg(test)]
pub struct LoudIsSpeech;

#[cfg(test)]
impl Vad for LoudIsSpeech {
    fn score(&mut self, frame: &[i16]) -> f32 {
        let loud = frame.iter().any(|sample| sample.unsigned_abs() > 1_000);
        if loud { 1.0 } else { 0.0 }
    }

    fn reset(&mut self) {}
}

/// `seconds` of 16 kHz audio at one level, for tests.
#[cfg(test)]
pub fn level(seconds: f64, value: i16) -> Vec<i16> {
    vec![value; (seconds * 16_000.0) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> SpeechGate {
        SpeechGate::new(Box::new(LoudIsSpeech))
    }

    #[test]
    fn silence_and_room_tone_are_not_speech() {
        let mut gate = gate();
        assert!(!gate.push(&level(30.0, 0)));
        assert!(!gate.push(&level(30.0, 200)));
    }

    #[test]
    fn a_second_of_talking_is_speech() {
        let mut gate = gate();
        assert!(gate.push(&level(1.0, 8_000)));
    }

    #[test]
    fn a_click_too_short_to_be_a_word_is_not() {
        let mut gate = gate();
        // 32 ms: two frames, under the segmenter's onset and minimum length.
        assert!(!gate.push(&level(0.032, 8_000)));
        assert!(!gate.push(&level(2.0, 0)));
    }

    /// The tee hands over blocks of any size (~341 frames at 48 kHz); the
    /// verdict must not depend on where they split.
    #[test]
    fn blocks_that_split_frames_give_the_same_answer() {
        let speech = level(1.0, 8_000);
        let mut gate = gate();
        let heard = speech
            .chunks(341)
            .fold(false, |heard, block| heard | gate.push(block));
        assert!(heard);
        assert!(gate.carry.len() < FRAME_SAMPLES);
    }

    #[test]
    fn the_real_detector_hears_no_speech_in_digital_silence() {
        let mut gate = SpeechGate::earshot();
        assert!(!gate.push(&level(5.0, 0)));
    }
}
