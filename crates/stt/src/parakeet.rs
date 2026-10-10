//! The Parakeet engine (TUR-62): NVIDIA `parakeet-tdt-0.6b-v3` on ONNX
//! Runtime, on the CPU, through `parakeet-rs`.
//!
//! The fast option for a Windows or Linux laptop with no graphics card, where
//! whisper on the CPU can fall behind a live call (SPEC §2.5 and findings D3
//! name it as the low-latency path). It runs on macOS too, but there Apple's
//! engine is the better default; nothing picks Parakeet unless config asks for
//! `"engine": "parakeet"`. `auto` does not consider it (yet).
//!
//! # Same guard as whisper
//!
//! Parakeet is a transducer, not a sequence-to-sequence model, so it does not
//! make up subtitle boilerplate the way whisper does. It still gets the same
//! guard, in the same order, so a meeting reads the same on either engine:
//!
//! 1. **VAD gating.** It only ever sees spans [`crate::vad::detect_speech`]
//!    (batch) or [`SpanAssembler`] (live) returned. Silence yields no spans,
//!    so the model is never called and no line can be written.
//! 2. **The phrase rule.** [`crate::whisper::is_hallucination`] drops a
//!    sentence that is nothing but a sound annotation or a stock phrase.
//!    Not whisper's bare one-word list ("Okay.", "So."): from Parakeet those
//!    are real replies and are kept.
//!
//! # One model, two tracks
//!
//! A meeting opens one session per track on one engine. The sessions share the
//! loaded model behind a mutex, so the two tracks take turns rather than
//! holding the 650 MB model in memory twice. Each call uses every core
//! ([`ParakeetConfig::threads`]), so taking turns costs little.
//!
//! # ONNX Runtime
//!
//! Linked into the binary on macOS and Linux; loaded from `onnxruntime.dll`
//! next to meet-ai.exe on Windows (`crate::platform`). Either way the engine
//! asks the platform to have it ready before the first ONNX call, so a missing
//! DLL is a typed error and never a panic inside `ort`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use parakeet_rs::{ExecutionConfig, ExecutionProvider, ParakeetTDT, TimestampMode, Transcriber};

use crate::session::{
    LiveEmitter, LiveListener, ReadySpan, SessionOptions, SessionOutcome, SpanAssembler, SttSession,
};
use crate::sink::TranscriptSink;
use crate::vad::{EarshotVad, SAMPLE_RATE, SegmentConfig, Vad, detect_speech};
use crate::whisper::is_hallucination;
use crate::{Error, Speaker, Utterance, collapse_whitespace};

/// Tunables for the Parakeet path.
#[derive(Debug, Clone)]
pub struct ParakeetConfig {
    /// ONNX Runtime threads per call. Defaults to every core the OS reports.
    pub threads: usize,
    /// How speech is cut into utterances. The same default as whisper's.
    pub segmentation: SegmentConfig,
}

impl Default for ParakeetConfig {
    fn default() -> Self {
        Self {
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            segmentation: SegmentConfig::default(),
        }
    }
}

/// The loaded model, shared by every session of one engine.
type SharedModel = Arc<Mutex<ParakeetTDT>>;

/// Parakeet behind the [`crate::SttEngine`] trait.
pub struct ParakeetEngine {
    model: SharedModel,
    vad: Box<dyn Vad>,
    config: ParakeetConfig,
    model_dir: PathBuf,
}

impl ParakeetEngine {
    pub const NAME: &'static str = crate::registry::PARAKEET;

    /// Load the model folder (`crate::model::parakeet::ParakeetModel::dir`).
    ///
    /// Nothing here touches the network: the files got there through
    /// `modelfetch`, as a separate step.
    pub fn load(model_dir: &Path, config: ParakeetConfig) -> Result<Self, Error> {
        if !model_dir.is_dir() {
            return Err(Error::ModelMissing(model_dir.to_path_buf()));
        }

        // Before any ONNX call: on Windows this loads onnxruntime.dll by its
        // full path, so `ort` never falls back to a bare-name lookup.
        crate::platform::prepare_onnx_runtime()?;

        // CPU only, on every OS. No DirectML: it crashes on old CPUs.
        let execution = ExecutionConfig::new()
            .with_execution_provider(ExecutionProvider::Cpu)
            .with_intra_threads(config.threads.max(1));
        let model = ParakeetTDT::from_pretrained(model_dir, Some(execution)).map_err(|e| {
            Error::Engine(format!(
                "could not load the Parakeet model in {}: {e}",
                model_dir.display()
            ))
        })?;

        Ok(Self {
            model: Arc::new(Mutex::new(model)),
            vad: Box::new(EarshotVad::new()),
            config,
            model_dir: model_dir.to_path_buf(),
        })
    }

    /// Swap the detector, as [`crate::whisper::WhisperEngine::with_vad`] does.
    pub fn with_vad(mut self, vad: Box<dyn Vad>) -> Self {
        self.vad = vad;
        self
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }
}

/// Take the model. A panic in another track's call leaves nothing half-done
/// that the next call depends on (each call starts from fresh features), so a
/// poisoned lock is recovered rather than ending the meeting's transcript.
fn lock(model: &Mutex<ParakeetTDT>) -> MutexGuard<'_, ParakeetTDT> {
    model
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `samples` as `f32` in `[-1, 1]`, padded with silence to at least one second.
///
/// The padding matches whisper's: the encoder subsamples 8 times, and a span a
/// fraction of a second long would otherwise reach it as a handful of frames.
fn to_parakeet_audio(samples: &[i16]) -> Vec<f32> {
    let mut audio: Vec<f32> = samples
        .iter()
        .map(|sample| *sample as f32 / i16::MAX as f32)
        .collect();
    if audio.len() < SAMPLE_RATE as usize {
        audio.resize(SAMPLE_RATE as usize, 0.0);
    }
    audio
}

/// Turn the sentences Parakeet heard in one span into transcript lines.
///
/// `sentences` are `(seconds into the span, text)`. Each one that survives the
/// phrase rule and is not blank becomes a line at `span_start_sec` plus its
/// offset. When the model gave text but no sentences, the whole text is one
/// line at the span's start.
fn lines(span_start_sec: f64, text: &str, sentences: &[(f32, String)]) -> Vec<(f64, String)> {
    let at = |offset: f64| (span_start_sec + offset).max(0.0);
    let candidates: Vec<(f64, &str)> = if sentences.is_empty() {
        vec![(at(0.0), text)]
    } else {
        sentences
            .iter()
            .map(|(offset, sentence)| (at(f64::from(*offset)), sentence.as_str()))
            .collect()
    };
    candidates
        .into_iter()
        .filter(|(_, raw)| {
            let dropped = is_hallucination(raw);
            if dropped {
                tracing::debug!(text = %raw, "dropped: known hallucination phrase");
            }
            !dropped
        })
        .filter_map(|(start, raw)| collapse_whitespace(raw).map(|text| (start, text)))
        .collect()
}

/// Run one VAD-approved span through Parakeet. The one place inference
/// happens, shared by the batch path and the live one, so both write the same
/// lines.
fn decode(
    model: &Mutex<ParakeetTDT>,
    samples: &[i16],
    span_start_sec: f64,
) -> Result<Vec<(f64, String)>, Error> {
    let result = lock(model)
        .transcribe_samples(
            to_parakeet_audio(samples),
            SAMPLE_RATE,
            1,
            Some(TimestampMode::Sentences),
        )
        .map_err(|e| Error::Engine(format!("Parakeet inference failed: {e}")))?;
    let sentences: Vec<(f32, String)> = result
        .tokens
        .into_iter()
        .map(|sentence| (sentence.start, sentence.text))
        .collect();
    Ok(lines(span_start_sec, &result.text, &sentences))
}

impl crate::SttEngine for ParakeetEngine {
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

        // Layer 1: no spans, no inference, nothing to write.
        for span in spans {
            for (start_sec, text) in decode(&self.model, span.samples(&pcm), span.start_sec())? {
                sink.write(&Utterance {
                    start_sec: start_sec as u64,
                    speaker,
                    text,
                })?;
            }
        }
        sink.flush()
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start_session(
        &mut self,
        options: SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn LiveListener>,
    ) -> Result<Box<dyn SttSession>, Error> {
        Ok(Box::new(ParakeetSession {
            model: Arc::clone(&self.model),
            assembler: SpanAssembler::with_default_vad(self.config.segmentation),
            emitter: LiveEmitter::new(&options, listener),
            sink,
        }))
    }
}

/// Parakeet over audio that is still arriving, one VAD-settled chunk at a
/// time, like [`crate::whisper::WhisperSession`] with live partials off: a
/// line appears when the speaker stops, never as a guess first.
pub struct ParakeetSession {
    model: SharedModel,
    assembler: SpanAssembler,
    emitter: LiveEmitter,
    sink: Box<dyn TranscriptSink + Send>,
}

impl ParakeetSession {
    fn settle(&mut self, span: &ReadySpan) -> Result<(), Error> {
        for (start_sec, text) in decode(&self.model, &span.samples, span.start_sec)? {
            self.emitter
                .finalize(start_sec, &text, self.sink.as_mut())?;
        }
        Ok(())
    }
}

impl SttSession for ParakeetSession {
    fn engine_name(&self) -> &'static str {
        ParakeetEngine::NAME
    }

    fn feed(&mut self, samples: &[i16]) -> Result<(), Error> {
        for span in self.assembler.push(samples) {
            self.settle(&span)?;
        }
        self.emitter.poll();
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<SessionOutcome, Error> {
        if let Some(span) = self.assembler.finish() {
            self.settle(&span)?;
        }
        let discarded_volatile = self.emitter.withdraw();
        self.sink.flush()?;
        Ok(SessionOutcome {
            speaker: self.emitter.speaker(),
            finalized: self.emitter.finalized(),
            discarded_volatile,
            audio_sec: self.assembler.fed_sec(),
            engine: ParakeetEngine::NAME,
        })
    }
}

/// Where to load ONNX Runtime from, when the platform loads it at run time.
///
/// `ORT_DYLIB_PATH` wins when it is set and not empty (the variable `ort`
/// itself reads), then `file_name` next to the executable. `None` when there
/// is neither. Always a full path when it comes from the executable's folder,
/// so the OS never searches its own folders for a library of that name.
///
/// OS-free, so every OS tests it; only Windows calls it.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "only the run-time-loading platform calls it")
)]
pub(crate) fn onnx_runtime_path(
    env_override: Option<OsString>,
    exe_dir: Option<&Path>,
    file_name: &str,
) -> Option<PathBuf> {
    if let Some(path) = env_override.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    exe_dir.map(|dir| dir.join(file_name))
}

/// Why ONNX Runtime at `path` cannot be loaded, in a sentence, or `None` when
/// the file is there. `None` for `path` means there was nowhere to look.
///
/// OS-free, so every OS tests it; only Windows calls it.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "only the run-time-loading platform calls it")
)]
pub(crate) fn onnx_runtime_problem(path: Option<&Path>, file_name: &str) -> Option<String> {
    match path {
        None => Some(format!(
            "the Parakeet engine needs ONNX Runtime ({file_name}), and meet-ai could not work \
             out its own folder to look for it"
        )),
        Some(path) if !path.is_file() => Some(format!(
            "the Parakeet engine needs ONNX Runtime ({file_name}), which is not installed at {}",
            path.display()
        )),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_is_scaled_like_whisper_and_padded_to_one_second() {
        let audio = to_parakeet_audio(&[0, i16::MAX, i16::MIN]);
        assert_eq!(audio.len(), SAMPLE_RATE as usize);
        assert_eq!(audio[0], 0.0);
        assert_eq!(audio[1], 1.0);
        assert_eq!(audio[2], i16::MIN as f32 / i16::MAX as f32);
        assert!(audio[3..].iter().all(|s| *s == 0.0));

        let long = vec![100i16; 3 * SAMPLE_RATE as usize];
        assert_eq!(to_parakeet_audio(&long).len(), long.len());
    }

    #[test]
    fn each_sentence_becomes_a_line_on_the_recording_timeline() {
        let found = lines(
            12.0,
            "Morning everyone. Let's start.",
            &[
                (0.5, "Morning everyone.".into()),
                (2.5, " Let's   start. ".into()),
            ],
        );
        assert_eq!(
            found,
            vec![
                (12.5, "Morning everyone.".to_string()),
                (14.5, "Let's start.".to_string())
            ]
        );
    }

    #[test]
    fn text_without_sentences_is_one_line_at_the_span_start() {
        assert_eq!(
            lines(3.0, "ship it on friday", &[]),
            vec![(3.0, "ship it on friday".to_string())]
        );
        assert!(lines(3.0, "   ", &[]).is_empty());
    }

    #[test]
    fn the_same_phrase_rule_as_whisper_drops_annotations_and_stock_phrases() {
        let found = lines(
            0.0,
            "",
            &[
                (0.0, "[BLANK_AUDIO]".into()),
                (0.5, "Thank you.".into()),
                (1.0, "Thank you, that fixes it.".into()),
                (2.0, "(water rushing)".into()),
            ],
        );
        assert_eq!(found, vec![(1.0, "Thank you, that fixes it.".to_string())]);
    }

    #[test]
    fn a_one_word_reply_is_kept() {
        let found = lines(0.0, "", &[(0.0, "Okay.".into()), (1.0, "Hmm.".into())]);
        assert_eq!(
            found,
            vec![(0.0, "Okay.".to_string()), (1.0, "Hmm.".to_string())]
        );
    }

    #[test]
    fn a_missing_model_folder_is_a_typed_error_before_onnx_is_touched() {
        let result = ParakeetEngine::load(
            Path::new("/nonexistent/parakeet-tdt-0.6b-v3-int8"),
            ParakeetConfig::default(),
        );
        match result {
            Err(Error::ModelMissing(path)) => {
                assert!(path.ends_with("parakeet-tdt-0.6b-v3-int8"));
            }
            Err(other) => panic!("expected ModelMissing, got {other:?}"),
            Ok(_) => panic!("loading a nonexistent model should not succeed"),
        }
    }

    #[test]
    fn a_folder_of_garbage_is_a_typed_error_not_a_panic() {
        // Gets past the folder check and into parakeet-rs and ONNX Runtime, so
        // it proves both link and run on every OS CI builds without the
        // 670 MB model. On an OS that loads ONNX Runtime at run time and has
        // none next to the test binary, the honest answer is that it is
        // missing, not a crash.
        let dir = tempfile::tempdir().unwrap();
        for name in ["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx"] {
            std::fs::write(dir.path().join(name), b"this is not a model").unwrap();
        }
        // A token list that parses, so the load reaches ONNX Runtime.
        std::fs::write(dir.path().join("vocab.txt"), "<unk> 0\n<blk> 1\n").unwrap();
        match ParakeetEngine::load(dir.path(), ParakeetConfig::default()) {
            Err(Error::Engine(message)) => {
                assert!(message.contains("could not load"), "got {message}");
            }
            Err(Error::EngineUnavailable(message))
                if crate::platform::ONNX_RUNTIME_LOADED_AT_RUN_TIME =>
            {
                assert!(message.contains("ONNX Runtime"), "got {message}");
            }
            Err(other) => panic!("expected an Engine error, got {other:?}"),
            Ok(_) => panic!("a garbage folder should not load as a model"),
        }
    }

    #[test]
    fn onnx_runtime_comes_from_the_override_then_next_to_the_app() {
        let exe = Path::new("/apps/meet-ai");
        assert_eq!(
            onnx_runtime_path(Some("/custom/ort.dll".into()), Some(exe), "onnxruntime.dll"),
            Some(PathBuf::from("/custom/ort.dll"))
        );
        // Empty means unset, as `ort` treats it.
        assert_eq!(
            onnx_runtime_path(Some("".into()), Some(exe), "onnxruntime.dll"),
            Some(exe.join("onnxruntime.dll"))
        );
        assert_eq!(
            onnx_runtime_path(None, Some(exe), "onnxruntime.dll"),
            Some(exe.join("onnxruntime.dll"))
        );
        assert_eq!(onnx_runtime_path(None, None, "onnxruntime.dll"), None);
    }

    #[test]
    fn a_missing_onnx_runtime_names_the_file_and_where_it_was_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("onnxruntime.dll");
        assert_eq!(
            onnx_runtime_problem(Some(&missing), "onnxruntime.dll"),
            Some(format!(
                "the Parakeet engine needs ONNX Runtime (onnxruntime.dll), which is not \
                 installed at {}",
                missing.display()
            ))
        );
        assert!(
            onnx_runtime_problem(None, "onnxruntime.dll")
                .unwrap()
                .contains("could not work out its own folder")
        );

        std::fs::write(&missing, b"stands in for the DLL").unwrap();
        assert_eq!(
            onnx_runtime_problem(Some(&missing), "onnxruntime.dll"),
            None
        );
    }
}
