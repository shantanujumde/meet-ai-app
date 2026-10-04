//! The OS seam for speech-to-text (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. SPEC §2.5 keeps
//! engine *selection* in [`crate::registry`] and platform-free; what differs per
//! OS is:
//!
//! * whether Apple's engine can exist at all ([`APPLE_SPEECH_UNSUPPORTED`]);
//! * how whisper is built: Metal on macOS (`macos.rs`), CPU everywhere else
//!   (`other.rs`, shared by Windows, Linux and any other OS). GPU off macOS is TUR-61. The
//!   cargo features behind that are in Cargo.toml; loading is the same code.

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

/// Load the whisper model at `model` — what [`crate::registry::select`] builds
/// for the whisper choice. The language comes from the model (TUR-94): `en`
/// for an English-only one, auto-detect for a multilingual one.
pub(crate) fn load_whisper(
    model: &std::path::Path,
) -> Result<Box<dyn crate::SttEngine>, crate::Error> {
    Ok(Box::new(whisper::WhisperEngine::load(
        model,
        recording_config(model),
    )?))
}

/// The config a real recording runs `model` with.
pub(crate) fn recording_config(model: &std::path::Path) -> whisper::WhisperConfig {
    whisper::WhisperConfig {
        language: crate::model::whisper_language_for_file(model).map(str::to_owned),
        ..whisper::WhisperConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn a_recording_tells_whisper_the_language_its_model_was_built_for() {
        for spec in crate::model::MODELS {
            let config = super::recording_config(&Path::new("/m").join(spec.filename));
            assert_eq!(
                config.language.as_deref(),
                spec.whisper_language(),
                "{}",
                spec.id
            );
        }
        let turbo = super::recording_config(Path::new("/m/ggml-large-v3-turbo-q5_0.bin"));
        assert_eq!(turbo.language, None);
    }
}

/// What `lib.rs` re-exports to other crates: `stt::whisper`.
pub mod public {
    pub use super::whisper;
}
