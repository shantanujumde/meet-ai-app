//! The OS seam for speech-to-text (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. SPEC §2.5 keeps
//! engine *selection* in [`crate::registry`] and platform-free; what differs per
//! OS is:
//!
//! * whether Apple's engine can exist at all ([`APPLE_SPEECH_UNSUPPORTED`]);
//! * how whisper is built: Metal on macOS (`macos.rs`), CPU everywhere else
//!   (`linux.rs`, shared by Windows and Linux). GPU off macOS is TUR-61. The
//!   cargo features behind that are in Cargo.toml; loading is the same code.

// `whisper.rs` stays where it was; only its declaration lives here, so
// `lib.rs` has no `cfg` and `stt::whisper` keeps its public path.
#[path = "../whisper.rs"]
pub mod whisper;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod linux;

#[cfg(target_os = "macos")]
use macos as os;

#[cfg(not(target_os = "macos"))]
use linux as os;

pub(crate) use os::APPLE_SPEECH_UNSUPPORTED;

/// Load the whisper model at `model` with the default config — what
/// [`crate::registry::select`] builds for the whisper choice.
pub(crate) fn load_whisper(
    model: &std::path::Path,
) -> Result<Box<dyn crate::SttEngine>, crate::Error> {
    Ok(Box::new(whisper::WhisperEngine::load(
        model,
        whisper::WhisperConfig::default(),
    )?))
}

/// What `lib.rs` re-exports to other crates: `stt::whisper`.
pub mod public {
    pub use super::whisper;
}
