//! The OS seam for speech-to-text (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. SPEC §2.5 keeps
//! engine *selection* in [`crate::registry`] and platform-free; what differs per
//! OS is:
//!
//! * whether Apple's engine can exist at all ([`APPLE_SPEECH_UNSUPPORTED`]);
//! * how whisper is built: Metal on macOS (`macos.rs`), Vulkan on Windows x64
//!   and Linux, CPU on any other OS (`other.rs`, TUR-61). The cargo features
//!   behind that are in Cargo.toml; loading is the same code;
//! * how to tell whether a GPU whisper can use is there, and the words for
//!   this machine in a recommendation.

// `whisper.rs` stays where it was; only its declaration lives here, so
// `lib.rs` has no `cfg` and `stt::whisper` keeps its public path.
#[path = "../whisper.rs"]
pub mod whisper;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(target_os = "macos")]
use macos as os;

#[cfg(not(target_os = "macos"))]
use other as os;

pub(crate) use os::APPLE_SPEECH_UNSUPPORTED;
// The hardware tier (TUR-61): whether a GPU whisper can use is there, and the
// words `model::recommended` names this machine and its GPU with.
pub(crate) use os::{GPU_NONE, GPU_YES, MACHINE, gpu_present};

// ONNX Runtime for the Parakeet engine (TUR-62): linked in on macOS and Linux,
// loaded from onnxruntime.dll at run time on Windows.
#[cfg(windows)]
#[path = "onnx_windows.rs"]
mod onnx;

#[cfg(not(windows))]
#[path = "onnx_linked.rs"]
mod onnx;

#[cfg(test)]
pub(crate) use onnx::ONNX_RUNTIME_LOADED_AT_RUN_TIME;
pub(crate) use onnx::{onnx_runtime_missing, prepare_onnx_runtime};

/// Load the whisper model at `model` — what [`crate::registry::select`] builds
/// for the whisper choice. An English-only model is told `en` (TUR-94); a
/// multilingual one is told what `spoken` says (`transcription.language`), and
/// works the language out line by line when that names none.
pub(crate) fn load_whisper(
    model: &std::path::Path,
    spoken: crate::languages::Spoken,
) -> Result<Box<dyn crate::SttEngine>, crate::Error> {
    Ok(Box::new(whisper::WhisperEngine::load(
        model,
        recording_config(model, spoken),
    )?))
}

/// The config a real recording runs `model` with.
pub(crate) fn recording_config(
    model: &std::path::Path,
    spoken: crate::languages::Spoken,
) -> whisper::WhisperConfig {
    // The model's own `en` wins: an English-only model cannot write Marathi.
    let own = crate::model::whisper_language_for_file(model);
    whisper::WhisperConfig {
        language: own.or(spoken.language).map(str::to_owned),
        prompt: own
            .is_none()
            .then_some(spoken.prompt)
            .flatten()
            .map(str::to_owned),
        // A recording always arms the GPU crash marker (TUR-61).
        gpu_guard: crate::gpu_guard::dir_for_model(model),
        ..whisper::WhisperConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::languages::{Spoken, spoken};

    #[test]
    fn a_recording_tells_whisper_the_language_its_model_was_built_for() {
        for spec in crate::model::MODELS {
            let config =
                super::recording_config(&Path::new("/m").join(spec.filename), Spoken::default());
            assert_eq!(
                config.language.as_deref(),
                spec.whisper_language(),
                "{}",
                spec.id
            );
        }
        let turbo = super::recording_config(
            Path::new("/m/ggml-large-v3-turbo-q5_0.bin"),
            Spoken::default(),
        );
        assert_eq!(turbo.language, None);
    }

    #[test]
    fn hinglish_is_english_with_a_prompt_on_a_multilingual_model_only() {
        let turbo = super::recording_config(
            Path::new("/m/ggml-large-v3-turbo-q5_0.bin"),
            spoken("hinglish"),
        );
        assert_eq!(turbo.language.as_deref(), Some("en"));
        assert!(turbo.prompt.is_some());
        let small_en =
            super::recording_config(Path::new("/m/ggml-small.en-q5_1.bin"), spoken("hinglish"));
        assert_eq!(small_en.prompt, None);
    }

    #[test]
    fn the_language_setting_reaches_a_multilingual_model_only() {
        let turbo = Path::new("/m/ggml-large-v3-turbo-q5_0.bin");
        let small_en = Path::new("/m/ggml-small.en-q5_1.bin");
        assert_eq!(
            super::recording_config(turbo, spoken("mr"))
                .language
                .as_deref(),
            Some("mr")
        );
        assert_eq!(
            super::recording_config(small_en, spoken("mr"))
                .language
                .as_deref(),
            Some("en")
        );
    }
}

/// What `lib.rs` re-exports to other crates: `stt::whisper`.
pub mod public {
    pub use super::whisper;
}
