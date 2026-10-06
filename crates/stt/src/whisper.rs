//! The `whisper-rs` fallback engine.
//!
//! The guaranteed floor everywhere Apple's engine is absent — Windows, and a
//! deliberate manual override on mac. Since SPEC A8 raised the mac floor to
//! macOS 26 it is no longer a *version* fallback there (SPEC §2.5).
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
use std::sync::{Arc, Once};

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    WhisperTokenId,
};

/// whisper.cpp installs its log callback globally, so this must happen once
/// per process no matter how many engines are constructed.
static INSTALL_LOGGING_HOOKS: Once = Once::new();

/// Route whisper.cpp's and ggml's logging through `tracing`, once per process.
/// Also called before anything else asks ggml a question
/// ([`crate::hardware::ggml_gpu_devices`]).
pub(crate) fn install_logging_hooks() {
    INSTALL_LOGGING_HOOKS.call_once(whisper_rs::install_logging_hooks);
}

use crate::gpu_guard::{self, GpuGuard};
use crate::session::{
    LiveEmitter, LiveListener, SessionOptions, SessionOutcome, SpanAssembler, SttSession,
};
use crate::sink::TranscriptSink;
use crate::spoken_language::SpokenLanguage;
use crate::vad::{EarshotVad, SAMPLE_RATE, SegmentConfig, Vad, detect_speech};
use crate::whisper_text::{collapse_repeats, echoes_prompt, prompt_tokens};
use crate::{Error, Speaker, Utterance, collapse_whitespace};

/// Layer 3 lives with whisper's other text repairs; it stays public here.
pub use crate::whisper_text::is_hallucination;

/// Tunables for the whisper path.
#[derive(Debug, Clone)]
pub struct WhisperConfig {
    /// `en` for the English-only models. `None` means auto-detect, learned
    /// per speaker over the meeting ([`SpokenLanguage`]) rather than guessed
    /// afresh for every utterance.
    pub language: Option<String>,
    /// Text whisper is told it has already heard, to steer how it writes:
    /// Hinglish's example sentences make it write Hindi in English letters
    /// instead of translating it. `None` for no prompt.
    pub prompt: Option<String>,
    /// Inference threads. Defaults to the physical core count.
    pub threads: i32,
    /// Drop any segment whose no-speech probability exceeds this.
    ///
    /// 0.6 is deliberately lenient: layer 1 has already established that this
    /// span contains speech, so this is a backstop, not the primary filter.
    pub no_speech_max: f32,
    /// How speech is cut into utterances.
    pub segmentation: SegmentConfig,

    /// Show a volatile hypothesis while someone is still talking.
    ///
    /// **Off by default, and that is the honest setting.** SPEC §5 Phase 2
    /// says "native streaming on macOS 26, *chunked* on the whisper path":
    /// whisper has no partial-result API, so the only way to guess at an
    /// unfinished sentence is to run a whole extra inference pass over the
    /// open span and throw the answer away when the real one arrives. On the
    /// fallback engine — the one already chosen because this Mac is slower or
    /// older — that roughly doubles the cost of transcription to draw a line
    /// that is about to be replaced. Turn it on when the machine can afford
    /// it; the [`crate::session`] tail contract is identical either way.
    pub live_partials: bool,

    /// Shortest open span worth guessing about, in seconds.
    ///
    /// whisper on 200 ms of audio produces noise, and noise on screen is
    /// worse than nothing there yet. Ignored unless [`Self::live_partials`].
    pub partial_min_sec: f64,

    /// The folder for the GPU crash marker ([`crate::gpu_guard`]), usually
    /// `.app/`. `None` (the default) starts whisper the way whisper-rs does
    /// with no marker: on the GPU when the build has one. A recording always
    /// sets it (`platform::recording_config`).
    pub gpu_guard: Option<PathBuf>,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            language: Some("en".into()),
            prompt: None,
            threads: std::thread::available_parallelism()
                .map(|n| n.get() as i32)
                .unwrap_or(4),
            no_speech_max: 0.6,
            segmentation: SegmentConfig::default(),
            live_partials: false,
            partial_min_sec: 1.0,
            gpu_guard: None,
        }
    }
}

/// whisper.cpp behind the [`crate::SttEngine`] trait.
pub struct WhisperEngine {
    context: WhisperContext,
    vad: Box<dyn Vad>,
    config: WhisperConfig,
    model_path: PathBuf,
    /// [`WhisperConfig::prompt`] as tokens, made once.
    prompt: Vec<WhisperTokenId>,
    /// The armed GPU crash marker, cleared by the first decode that works.
    gpu: Option<Arc<GpuGuard>>,
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
        install_logging_hooks();

        // `use_gpu` defaults to whether this build has a GPU backend (Metal
        // on macOS, Vulkan on Windows x64 and Linux). With no usable device
        // whisper.cpp stays on the CPU by itself; the marker is for a GPU that
        // is there and crashes.
        let mut params = WhisperContextParameters::default();
        let gpu = match &config.gpu_guard {
            Some(dir) if params.use_gpu => {
                let decision = gpu_guard::arm(dir);
                params.use_gpu = decision.use_gpu;
                decision.guard
            }
            _ => None,
        };
        tracing::info!(use_gpu = params.use_gpu, model = %model_path.display(), "loading whisper");

        let context = WhisperContext::new_with_params(model_path, params)
            .map_err(|e| Error::Engine(format!("could not load {}: {e}", model_path.display())))?;
        // A load that fails returns here and drops `gpu`, which clears the
        // marker: an error is not a crash.
        let prompt = prompt_tokens(&context, config.prompt.as_deref())?;

        Ok(Self {
            context,
            prompt,
            vad: Box::new(EarshotVad::new()),
            config,
            model_path: model_path.to_path_buf(),
            gpu,
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

    fn new_state(&self) -> Result<WhisperState, Error> {
        self.context
            .create_state()
            .map_err(|e| Error::Engine(format!("could not create whisper state: {e}")))
    }
}

fn params<'a>(
    config: &WhisperConfig,
    language: Option<&'a str>,
    prompt: &'a [WhisperTokenId],
) -> FullParams<'a, 'a> {
    // Greedy with no beam search: this is the fallback engine, and the
    // accuracy gain from beams costs more time than the whole Apple path
    // takes end to end.
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(config.threads);
    params.set_language(language);
    params.set_tokens(prompt);
    params.set_translate(false);

    // Quiet. whisper.cpp prints to stdout by default, which would corrupt
    // anything else parsing this process's output.
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // Hallucination guard, layer 2.
    params.set_no_speech_thold(config.no_speech_max);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true); // suppress non-speech tokens
    // Temperature 0 with no fallback: the temperature-increase retry loop
    // is what turns a low-confidence span into confident nonsense.
    params.set_temperature(0.0);
    params.set_temperature_inc(0.0);

    params
}

/// Fill `audio` with `samples` as `f32` in `[-1, 1]`, replacing what was there.
///
/// whisper.cpp refuses anything under ~1 s of audio, so shorter input is padded
/// with silence rather than skipped, or short real words get dropped. `audio`
/// is the caller's scratch, so repeated calls reuse its capacity.
fn to_whisper_audio(samples: &[i16], audio: &mut Vec<f32>) {
    audio.clear();
    audio.extend(
        samples
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32),
    );
    if audio.len() < SAMPLE_RATE as usize {
        audio.resize(SAMPLE_RATE as usize, 0.0);
    }
}

/// Run one VAD-approved span through whisper and return what survives layers
/// 2 and 3 of the hallucination guard.
///
/// The single place inference happens, shared by the batch path and the live
/// one. If the two filtered differently, a meeting would read one way on
/// screen and another way in `transcript.md`.
///
/// `audio` comes from [`to_whisper_audio`]; `language` is what to tell
/// whisper, `None` to let it guess, and `prompt` is [`WhisperConfig::prompt`]
/// as tokens. `span_start_sec` positions the result on
/// the recording's timeline: whisper reports centiseconds relative to the clip
/// it was handed.
fn decode(
    state: &mut WhisperState,
    config: &WhisperConfig,
    audio: &[f32],
    language: Option<&str>,
    prompt: &[WhisperTokenId],
    span_start_sec: f64,
    gpu: Option<&GpuGuard>,
) -> Result<Vec<(f64, String)>, Error> {
    state
        .full(params(config, language, prompt), audio)
        .map_err(|e| Error::Engine(format!("whisper inference failed: {e}")))?;
    // The first decode that came back is the GPU working: clear the marker.
    if let Some(gpu) = gpu {
        gpu.passed();
    }

    let mut lines = Vec::new();
    for segment in state.as_iter() {
        let Ok(raw) = segment.to_str_lossy() else {
            continue;
        };

        // Layer 2.
        if segment.no_speech_probability() > config.no_speech_max {
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

        let Some(text) = collapse_whitespace(&collapse_repeats(&raw)) else {
            continue;
        };
        if config
            .prompt
            .as_deref()
            .is_some_and(|prompt| echoes_prompt(&text, prompt))
        {
            tracing::debug!(text = %text, "dropped: the prompt written back");
            continue;
        }

        let within_span = segment.start_timestamp() as f64 / 100.0;
        lines.push(((span_start_sec + within_span).max(0.0), text));
    }

    Ok(lines)
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

        let mut state = self.new_state()?;
        let mut audio = Vec::new();
        // One file is one speaker, so it learns its own language.
        let mut heard = SpokenLanguage::new();

        for span in spans {
            let samples = span.samples(&pcm);
            to_whisper_audio(samples, &mut audio);
            let span_sec = samples.len() as f64 / SAMPLE_RATE as f64;
            let language = self
                .config
                .language
                .as_deref()
                .or_else(|| heard.learn(&mut state, self.config.threads, &audio, span_sec));
            for (start_sec, text) in decode(
                &mut state,
                &self.config,
                &audio,
                language,
                &self.prompt,
                span.start_sec(),
                self.gpu.as_deref(),
            )? {
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
        Ok(Box::new(WhisperSession {
            state: self.new_state()?,
            config: self.config.clone(),
            assembler: SpanAssembler::with_default_vad(self.config.segmentation),
            emitter: LiveEmitter::new(&options, listener),
            sink,
            audio: Vec::new(),
            heard: SpokenLanguage::new(),
            prompt: self.prompt.clone(),
            gpu: self.gpu.clone(),
        }))
    }
}

/// whisper over audio that is still arriving, one VAD-settled chunk at a time.
///
/// "Chunked", in SPEC §5's sense: there is no partial-result API in whisper, so
/// an utterance becomes a line when the detector says the speaker stopped, not
/// token by token. [`WhisperConfig::live_partials`] buys a hypothesis in
/// between at the price of a second inference pass.
///
/// The silence gate is not re-implemented here. It is [`SpanAssembler`], which
/// is [`crate::vad::detect_speech`] with the audio arriving late: no spans over
/// quiet audio means [`decode`] is never called, which means there is nothing
/// for whisper to invent a line out of.
pub struct WhisperSession {
    state: WhisperState,
    config: WhisperConfig,
    assembler: SpanAssembler,
    emitter: LiveEmitter,
    sink: Box<dyn TranscriptSink + Send>,
    /// Scratch for the `f32` copy whisper takes, reused by every decode.
    audio: Vec<f32>,
    /// This session's speaker's languages, when the config names none.
    heard: SpokenLanguage,
    /// [`WhisperConfig::prompt`] as tokens, from the engine.
    prompt: Vec<WhisperTokenId>,
    /// The engine's GPU crash marker, cleared by the first decode that works.
    gpu: Option<Arc<GpuGuard>>,
}

impl WhisperSession {
    /// Settle a span into transcript lines.
    fn settle(&mut self, span: &crate::session::ReadySpan) -> Result<(), Error> {
        to_whisper_audio(&span.samples, &mut self.audio);
        let span_sec = span.samples.len() as f64 / SAMPLE_RATE as f64;
        let language = self.config.language.as_deref().or_else(|| {
            self.heard
                .learn(&mut self.state, self.config.threads, &self.audio, span_sec)
        });
        for (start_sec, text) in decode(
            &mut self.state,
            &self.config,
            &self.audio,
            language,
            &self.prompt,
            span.start_sec,
            self.gpu.as_deref(),
        )? {
            self.emitter
                .finalize(start_sec, &text, self.sink.as_mut())?;
        }
        Ok(())
    }

    /// Guess at the utterance in progress, if the caller is paying for that.
    fn guess(&mut self) -> Result<(), Error> {
        if !self.config.live_partials || !self.emitter.wants_volatile() {
            return Ok(());
        }
        let Some((start_sec, samples)) = self.assembler.open_view() else {
            return Ok(());
        };
        if (samples.len() as f64 / SAMPLE_RATE as f64) < self.config.partial_min_sec {
            return Ok(());
        }

        // A guess is thrown away, so it does not teach the language anything.
        to_whisper_audio(samples, &mut self.audio);
        let language = self
            .config
            .language
            .as_deref()
            .or_else(|| self.heard.usual_code());
        let guessed = decode(
            &mut self.state,
            &self.config,
            &self.audio,
            language,
            &self.prompt,
            0.0,
            self.gpu.as_deref(),
        )?;
        // One tail per speaker, so several segments over one open span are one
        // hypothesis. An empty result withdraws the tail rather than freezing
        // the last guess on screen — whisper deciding the span is not speech
        // after all has to be visible.
        let text = guessed
            .iter()
            .map(|(_, line)| line.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        self.emitter.volatile(start_sec, &text);
        Ok(())
    }
}

impl SttSession for WhisperSession {
    fn engine_name(&self) -> &'static str {
        WhisperEngine::NAME
    }

    fn feed(&mut self, samples: &[i16]) -> Result<(), Error> {
        for span in self.assembler.push(samples) {
            self.settle(&span)?;
        }
        self.guess()?;
        self.emitter.poll();
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<SessionOutcome, Error> {
        if let Some(span) = self.assembler.finish() {
            self.settle(&span)?;
        }

        // Whatever was still a guess stays a guess. `transcript.md` is
        // append-only, so there is no version of promoting it that is not a
        // line the user cannot get rid of.
        let discarded_volatile = self.emitter.withdraw();
        self.sink.flush()?;

        Ok(SessionOutcome {
            speaker: self.emitter.speaker(),
            finalized: self.emitter.finalized(),
            discarded_volatile,
            audio_sec: self.assembler.fed_sec(),
            engine: WhisperEngine::NAME,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_conversion_is_pinned_and_padded_to_one_second() {
        let mut audio = Vec::new();
        to_whisper_audio(&[0, i16::MAX, i16::MIN, 16_384], &mut audio);
        assert_eq!(audio.len(), SAMPLE_RATE as usize);
        assert_eq!(audio[0], 0.0);
        assert_eq!(audio[1], 1.0);
        assert_eq!(audio[2], i16::MIN as f32 / i16::MAX as f32);
        assert_eq!(audio[3], 16_384.0 / i16::MAX as f32);
        assert!(audio[4..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_audio_scratch_is_reused_and_never_leaks_old_samples() {
        let mut audio = Vec::new();
        let long = vec![1_000i16; 5 * SAMPLE_RATE as usize];
        to_whisper_audio(&long, &mut audio);
        let capacity = audio.capacity();
        to_whisper_audio(&long, &mut audio);
        to_whisper_audio(&[7; 10], &mut audio);
        assert_eq!(audio.len(), SAMPLE_RATE as usize);
        assert!(audio[10..].iter().all(|s| *s == 0.0), "stale audio leaked");
        assert_eq!(audio.capacity(), capacity);
    }

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

    #[test]
    fn a_file_that_is_not_a_model_is_a_typed_error_from_whisper_cpp() {
        // Unlike the test above, this gets past the `is_file` check and into
        // whisper.cpp itself, so it proves the C library links and runs on
        // every OS CI builds (Windows and Linux since TUR-52) without needing
        // a real model download.
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("ggml-not-a-model.bin");
        std::fs::write(&bogus, b"this is not a ggml model file").unwrap();
        match WhisperEngine::load(&bogus, WhisperConfig::default()) {
            Err(Error::Engine(message)) => {
                assert!(message.contains("could not load"), "got {message}");
            }
            Err(other) => panic!("expected Engine error, got {other:?}"),
            Ok(_) => panic!("a garbage file should not load as a model"),
        }
    }
}
