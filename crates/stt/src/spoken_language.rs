//! Which language each line is in, when `transcription.language` is `auto`.
//!
//! whisper is asked about every line long enough to judge, and a line it is
//! sure about is written in that language. That keeps a mixed call mixed: on a
//! real Hindi and English call, whisper was 80% sure of Hindi on the Hindi
//! lines and 100% sure of English on the English ones. Locking each speaker to
//! one language (what this first did) wrote that call as English and
//! translated every Hindi sentence.
//!
//! What it does keep per speaker (one [`SpokenLanguage`] per channel) is the
//! **usual** language: whisper's probabilities from every line of
//! [`MIN_EVIDENCE_SEC`] or more, weighted by length. A line too short to judge
//! ("Hmm", "Yeah", a cough), or one whisper is unsure about (under
//! [`CONFIDENT`]), is written in the usual language instead of a stray guess.
//! Before this, a Marathi call came out as lines of Chinese, Tamil, Russian and
//! Portuguese, nearly all of them from short or unsure lines.
//!
//! It cannot find a language whisper does not recognise. On that Marathi call,
//! large-v3-turbo never put Marathi in its top four, even for 10-second
//! sentences. Choosing the language in Settings is the fix for those.
//!
//! The bookkeeping is pure, over language ids, so it is tested without a
//! model. [`SpokenLanguage::learn`] is the one place that asks whisper.

use whisper_rs::WhisperState;

/// Lines shorter than this are not asked about, and teach nothing: whisper
/// guesses wildly on them, and asking costs a pass of the encoder.
pub const MIN_EVIDENCE_SEC: f64 = 2.0;

/// How sure whisper must be for a line to be written in its own language
/// rather than the speaker's usual one.
pub const CONFIDENT: f32 = 0.5;

/// whisper only looks at the first 30 seconds of what it is handed, so a
/// longer line does not count for more than that.
const MAX_WEIGHT_SEC: f64 = 30.0;

/// One speaker's languages over a meeting. See the module docs.
#[derive(Debug, Clone, Default)]
pub struct SpokenLanguage {
    /// whisper's probability for each language id, times seconds of speech.
    evidence: Vec<f64>,
}

impl SpokenLanguage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Is a line of `span_sec` worth asking whisper about?
    pub fn wants_detection(&self, span_sec: f64) -> bool {
        span_sec >= MIN_EVIDENCE_SEC
    }

    /// The language id to write a line in, given whisper's `probabilities` for
    /// it (indexed by language id). Long lines also count towards the usual
    /// language.
    pub fn choose(&mut self, probabilities: &[f32], span_sec: f64) -> Option<usize> {
        if !self.wants_detection(span_sec) {
            return self.usual();
        }
        let weight = span_sec.min(MAX_WEIGHT_SEC);
        if self.evidence.len() < probabilities.len() {
            self.evidence.resize(probabilities.len(), 0.0);
        }
        for (total, p) in self.evidence.iter_mut().zip(probabilities) {
            if p.is_finite() {
                *total += f64::from(*p) * weight;
            }
        }
        match top(probabilities) {
            Some((id, p)) if p >= CONFIDENT => Some(id),
            _ => self.usual(),
        }
    }

    /// The language with the most evidence so far. `None` before any, which
    /// leaves whisper to guess.
    pub fn usual(&self) -> Option<usize> {
        self.evidence
            .iter()
            .enumerate()
            .filter(|(_, total)| **total > 0.0)
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(id, _)| id)
    }

    /// The whisper code to write one line in. `audio` is what whisper is about
    /// to decode, `span_sec` of speech before padding.
    ///
    /// A failed ask is logged and the line goes out in the usual language: a
    /// recording is not worth losing over it.
    pub fn learn(
        &mut self,
        state: &mut WhisperState,
        threads: i32,
        audio: &[f32],
        span_sec: f64,
    ) -> Option<&'static str> {
        if !self.wants_detection(span_sec) {
            return self.usual_code();
        }
        let threads = threads.max(1) as usize;
        let detected = state
            .pcm_to_mel(audio, threads)
            .and_then(|()| state.lang_detect(0, threads));
        match detected {
            Ok((_, probabilities)) => self.choose(&probabilities, span_sec).and_then(code),
            Err(e) => {
                tracing::warn!(error = %e, "whisper language detection failed");
                self.usual_code()
            }
        }
    }

    /// The whisper code for [`Self::usual`], such as `hi`.
    pub fn usual_code(&self) -> Option<&'static str> {
        self.usual().and_then(code)
    }
}

/// whisper's code for language `id`.
fn code(id: usize) -> Option<&'static str> {
    whisper_rs::get_lang_str(i32::try_from(id).ok()?)
}

/// The most likely language in one set of probabilities.
fn top(probabilities: &[f32]) -> Option<(usize, f32)> {
    probabilities
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, p)| p.is_finite())
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: usize = 0;
    const ZH: usize = 1;
    const PT: usize = 8;
    const HI: usize = 17;
    const MR: usize = 60;

    /// Probabilities that give `id` `p` and the rest to `other`.
    fn guess(id: usize, p: f32, other: usize) -> Vec<f32> {
        let mut probabilities = vec![0.0; 100];
        probabilities[id] = p;
        probabilities[other] += 1.0 - p;
        probabilities
    }

    #[test]
    fn with_no_evidence_whisper_is_left_to_guess() {
        let language = SpokenLanguage::new();
        assert_eq!(language.usual(), None);
        assert!(language.wants_detection(MIN_EVIDENCE_SEC));
        assert!(!language.wants_detection(1.0));
    }

    #[test]
    fn a_mixed_call_keeps_each_line_in_its_own_language() {
        // Measured on a real Hindi and English call.
        let mut language = SpokenLanguage::new();
        assert_eq!(language.choose(&guess(EN, 1.0, HI), 4.8), Some(EN));
        assert_eq!(language.choose(&guess(HI, 0.57, EN), 4.0), Some(HI));
        assert_eq!(language.choose(&guess(EN, 0.97, HI), 3.0), Some(EN));
        assert_eq!(language.choose(&guess(HI, 0.85, EN), 3.2), Some(HI));
    }

    #[test]
    fn short_lines_follow_the_usual_language_and_teach_nothing() {
        let mut language = SpokenLanguage::new();
        language.choose(&guess(MR, 0.6, HI), 6.0);
        // "Once.", heard as Chinese, 1.4 seconds long.
        assert_eq!(language.choose(&guess(ZH, 0.9, EN), 1.4), Some(MR));
        for _ in 0..30 {
            language.choose(&guess(EN, 0.9, ZH), 0.8);
        }
        assert_eq!(language.usual(), Some(MR));
    }

    #[test]
    fn an_unsure_line_follows_the_usual_language() {
        let mut language = SpokenLanguage::new();
        language.choose(&guess(EN, 1.0, HI), 5.0);
        language.choose(&guess(EN, 1.0, HI), 5.0);
        // en=0.35 pt=0.34: a coin toss, so not a reason to switch.
        let mut unsure = guess(EN, 0.35, PT);
        unsure[PT] = 0.34;
        unsure[ZH] = 0.31;
        assert_eq!(language.choose(&unsure, 2.5), Some(EN));
    }

    #[test]
    fn the_usual_language_follows_the_weight_of_speech() {
        let mut language = SpokenLanguage::new();
        language.choose(&guess(EN, 0.9, HI), 2.0);
        language.choose(&guess(HI, 0.8, EN), 10.0);
        assert_eq!(language.usual(), Some(HI));
    }

    #[test]
    fn broken_probabilities_are_ignored() {
        let mut language = SpokenLanguage::new();
        let mut probabilities = guess(MR, 0.8, EN);
        probabilities[ZH] = f32::NAN;
        assert_eq!(language.choose(&probabilities, 5.0), Some(MR));
        assert_eq!(top(&[f32::NAN, 0.2, 0.1]), Some((1, 0.2)));
        assert_eq!(top(&[]), None);
    }
}
