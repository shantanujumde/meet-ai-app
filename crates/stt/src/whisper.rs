//! The `whisper-rs` fallback engine.
//!
//! Used on macOS 14.4–25, and the guaranteed floor everywhere else (SPEC §2.5).
//! Slower and less accurate than Apple's engine, and — the part that matters —
//! it hallucinates.
//!
//! # The hallucination guard
//!
//! This is the canonical failure of Phase 1: whisper handed 30 quiet seconds
//! emits "Thank you." Three independent layers stop it, in this order:
//!
//! 1. **VAD gating.** [`crate::vad::detect_speech`] decides which sample ranges
//!    whisper is allowed to see. Silence yields zero spans, so whisper is never
//!    invoked at all. This is the layer that actually does the work; the other
//!    two exist because this one is a judgement call, not a proof.
//! 2. **`no_speech_thold` + `suppress_nst`.** whisper.cpp's own per-segment
//!    no-speech probability, checked against [`WhisperConfig::no_speech_max`].
//! 3. **A phrase blocklist.** The handful of strings whisper reaches for when
//!    it has nothing — mostly YouTube subtitle boilerplate baked into its
//!    training data. Only applied to segments that are *entirely* one of these
//!    phrases, so a real "Thank you." inside a sentence survives.
//!
//! Layer 3 is deliberately last and deliberately narrow. A blocklist that ate
//! real speech would be a worse bug than the one it fixes.

use std::path::{Path, PathBuf};
use std::sync::Once;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// whisper.cpp installs its log callback globally, so this must happen once
/// per process no matter how many engines are constructed.
static INSTALL_LOGGING_HOOKS: Once = Once::new();

use crate::sink::TranscriptSink;
use crate::vad::{EarshotVad, SAMPLE_RATE, SegmentConfig, Vad, detect_speech};
use crate::{Error, Speaker, Utterance, collapse_whitespace};

/// Phrases whisper invents over quiet audio.
///
/// Compared case-insensitively against the whole trimmed segment, with
/// punctuation stripped. A segment equal to one of these and nothing else is
/// dropped. Sourced from the widely reported whisper.cpp hallucination set —
/// these are subtitle-corpus artefacts, not English that shows up alone in a
/// meeting.
const HALLUCINATION_PHRASES: &[&str] = &[
    "thank you",
    "thanks for watching",
    "thank you for watching",
    "thanks for watching!",
    "you",
    "bye",
    "bye bye",
    "thank you very much",
    "please subscribe",
    "subscribe to my channel",
    "blank_audio",
    "silence",
    "music",
    "applause",
    "inaudible",
    "beep",
    "so",
    "okay",
    "oh",
    "hmm",
];

/// Is this segment nothing but a known hallucination?
///
/// Two rules, in order:
///
/// 1. **Shape.** A segment that is entirely wrapped in `[...]`, `(...)` or
///    `*...*` is a sound annotation, not speech — `[BLANK_AUDIO]`,
///    `[no speech detected]`, `(water rushing)`, `*door closes*`. This rule is
///    the important one because it is structural: it catches annotations
///    nobody has seen yet, which an enumerated list by definition cannot.
///    Measured against this repo's fixtures, ungated whisper emitted
///    `[BLANK_AUDIO]`, `[no speech detected]` and `(water rushing)` over
///    silence and pink noise — only the first was on the list below.
/// 2. **A phrase list**, for bare-text hallucinations that carry no brackets.
///
/// Public so the test suite can assert the rule directly, and so a future
/// Silero swap can reuse it unchanged.
pub fn is_hallucination(text: &str) -> bool {
    let trimmed = text.trim();

    // Rule 1. Checked before punctuation is stripped, because the brackets
    // are the entire signal.
    let wrapped = [('[', ']'), ('(', ')'), ('*', '*'), ('<', '>'), ('{', '}')]
        .iter()
        .any(|(open, close)| {
            trimmed.starts_with(*open)
                && trimmed.ends_with(*close)
                && trimmed.chars().count() >= 2
                // Only if there is exactly one bracketed run, so a real
                // sentence like "(see the ticket) and then we ship" is kept.
                && !trimmed[1..trimmed.len() - close.len_utf8()].contains(*close)
        });
    if wrapped {
        return true;
    }

    let normalized: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
        .collect();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.is_empty() {
        return true;
    }
    HALLUCINATION_PHRASES.contains(&normalized.as_str())
}

/// Tunables for the whisper path.
#[derive(Debug, Clone)]
pub struct WhisperConfig {
    /// `en` for the English-only models; `None` asks whisper to auto-detect.
    pub language: Option<String>,
    /// Inference threads. Defaults to the physical core count.
    pub threads: i32,
    /// Drop any segment whose no-speech probability exceeds this.
    ///
    /// 0.6 is deliberately lenient: layer 1 has already established that this
    /// span contains speech, so this is a backstop, not the primary filter.
    pub no_speech_max: f32,
    /// How speech is cut into utterances.
    pub segmentation: SegmentConfig,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            language: Some("en".into()),
            threads: std::thread::available_parallelism()
                .map(|n| n.get() as i32)
                .unwrap_or(4),
            no_speech_max: 0.6,
            segmentation: SegmentConfig::default(),
        }
    }
}

/// whisper.cpp behind the [`crate::SttEngine`] trait.
pub struct WhisperEngine {
    context: WhisperContext,
    vad: Box<dyn Vad>,
    config: WhisperConfig,
    model_path: PathBuf,
}

impl WhisperEngine {
    pub const NAME: &'static str = crate::registry::WHISPER;

    /// Load a GGML model from disk.
    ///
    /// Nothing here touches the network. Getting the file there is
    /// [`crate::model`]'s job, and it is a separate, explicit step.
    pub fn load(model_path: &Path, config: WhisperConfig) -> Result<Self, Error> {
        if !model_path.is_file() {
            return Err(Error::ModelMissing(model_path.to_path_buf()));
        }

        // whisper.cpp and ggml print progress and diagnostics straight to
        // stdout/stderr by default ("single timestamp ending - skip entire
        // chunk", backend registration banners, and so on). Inside the Tauri
        // app that is noise in the log file; inside any CLI that speaks a line
        // protocol it is corruption. The `tracing_backend` feature routes them
        // through `tracing` instead, but only once this is called.
        INSTALL_LOGGING_HOOKS.call_once(whisper_rs::install_logging_hooks);
        let context =
            WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
                .map_err(|e| {
                    Error::Engine(format!("could not load {}: {e}", model_path.display()))
                })?;

        Ok(Self {
            context,
            vad: Box::new(EarshotVad::new()),
            config,
            model_path: model_path.to_path_buf(),
        })
    }

    /// Swap the detector — the seam SETUP.md §1.1 reserved for Silero.
    pub fn with_vad(mut self, vad: Box<dyn Vad>) -> Self {
        self.vad = vad;
        self
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    fn params(&self) -> FullParams<'_, '_> {
        // Greedy with no beam search: this is the fallback engine, and the
        // accuracy gain from beams costs more time than the whole Apple path
        // takes end to end.
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.config.threads);
        params.set_language(self.config.language.as_deref());
        params.set_translate(false);

        // Quiet. whisper.cpp prints to stdout by default, which would corrupt
        // anything else parsing this process's output.
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Hallucination guard, layer 2.
        params.set_no_speech_thold(self.config.no_speech_max);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true); // suppress non-speech tokens
        // Temperature 0 with no fallback: the temperature-increase retry loop
        // is what turns a low-confidence span into confident nonsense.
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);

        params
    }
}

impl crate::SttEngine for WhisperEngine {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn transcribe(
        &mut self,
        wav: &Path,
        speaker: Speaker,
        sink: &mut dyn TranscriptSink,
    ) -> Result<(), Error> {
        let pcm = crate::read_wav_16k_mono(wav)?;
        let spans = detect_speech(&pcm, self.vad.as_mut(), &self.config.segmentation);

        tracing::debug!(
            wav = %wav.display(),
            samples = pcm.len(),
            spans = spans.len(),
            "vad segmentation complete"
        );

        // Hallucination guard, layer 1: no spans means the loop body never
        // runs, so whisper is never called and nothing can be invented.
        if spans.is_empty() {
            return sink.flush();
        }

        let mut state = self
            .context
            .create_state()
            .map_err(|e| Error::Engine(format!("could not create whisper state: {e}")))?;

        for span in spans {
            let samples = span.samples(&pcm);
            let audio: Vec<f32> = samples
                .iter()
                .map(|sample| *sample as f32 / i16::MAX as f32)
                .collect();

            // whisper.cpp refuses anything under ~1 s of audio. Pad with
            // silence rather than skipping, or short real words get dropped.
            let audio = if audio.len() < SAMPLE_RATE as usize {
                let mut padded = audio;
                padded.resize(SAMPLE_RATE as usize, 0.0);
                padded
            } else {
                audio
            };

            state
                .full(self.params(), &audio)
                .map_err(|e| Error::Engine(format!("whisper inference failed: {e}")))?;

            for segment in state.as_iter() {
                let Ok(raw) = segment.to_str_lossy() else {
                    continue;
                };

                // Layer 2.
                if segment.no_speech_probability() > self.config.no_speech_max {
                    tracing::debug!(
                        text = %raw,
                        no_speech = segment.no_speech_probability(),
                        "dropped: no-speech probability above threshold"
                    );
                    continue;
                }

                // Layer 3.
                if is_hallucination(&raw) {
                    tracing::debug!(text = %raw, "dropped: known hallucination phrase");
                    continue;
                }

                let Some(text) = collapse_whitespace(&raw) else {
                    continue;
                };

                // whisper's own timestamps are relative to the span it was
                // given, in centiseconds. Offset them back onto the recording's
                // timeline so the transcript line is right.
                let within_span = segment.start_timestamp() as f64 / 100.0;
                let start_sec = span.start_sec() + within_span;

                sink.write(&Utterance {
                    start_sec: start_sec.max(0.0) as u64,
                    speaker,
                    text,
                })?;
            }
        }

        sink.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canonical_hallucinations_are_caught() {
        for phrase in [
            "Thank you.",
            "thank you",
            " Thank you. ",
            "Thanks for watching!",
            "You",
            "...",
            "   ",
        ] {
            assert!(is_hallucination(phrase), "{phrase:?} should be dropped");
        }
    }

    #[test]
    fn bracketed_sound_annotations_are_dropped_by_shape() {
        // Every one of these was actually produced by ungated whisper against
        // this repo's silence and pink-noise fixtures. Only the first was on
        // the phrase list, which is why the shape rule exists.
        for phrase in [
            "[BLANK_AUDIO]",
            " [no speech detected]",
            "(water rushing)",
            "*door closes*",
            "(applause)",
            "[Music]",
        ] {
            assert!(is_hallucination(phrase), "{phrase:?} should be dropped");
        }
    }

    #[test]
    fn a_sentence_that_merely_contains_brackets_is_kept() {
        for phrase in [
            "(see the ticket) and then we ship it",
            "The array [0] is empty",
            "Use (a) the cache or (b) Redis",
        ] {
            assert!(!is_hallucination(phrase), "{phrase:?} should be kept");
        }
    }

    #[test]
    fn real_speech_containing_a_blocked_phrase_survives() {
        // The blocklist matches whole segments only. If it matched substrings
        // it would eat real sentences, which is a worse bug than the one it is
        // there to fix.
        for phrase in [
            "Thank you, that fixes it.",
            "Okay, let's put that in a ticket.",
            "So the sessions are still in memory.",
            "Bye for now, I'll send the notes over.",
        ] {
            assert!(!is_hallucination(phrase), "{phrase:?} should be kept");
        }
    }

    #[test]
    fn a_missing_model_is_a_typed_error_not_a_panic() {
        // `WhisperEngine` holds a raw whisper.cpp context and is not `Debug`,
        // so match on the result rather than reaching for `unwrap_err`.
        let result = WhisperEngine::load(
            Path::new("/nonexistent/ggml-small.en-q5_1.bin"),
            WhisperConfig::default(),
        );
        match result {
            Err(Error::ModelMissing(path)) => {
                assert!(path.ends_with("ggml-small.en-q5_1.bin"));
            }
            Err(other) => panic!("expected ModelMissing, got {other:?}"),
            Ok(_) => panic!("loading a nonexistent model should not succeed"),
        }
    }
}
