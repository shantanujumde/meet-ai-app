//! The speech seam on Windows: whisper is the only engine, built for the CPU.
//!
//! whisper.cpp is compiled with `GGML_NATIVE=OFF` (cmake/force-portable-ggml.cmake)
//! so the binary runs on any x86-64 CPU, not only the one it was built on.
//! GPU (`vulkan`/`cuda`) is TUR-61.

use std::path::Path;

use super::whisper::{WhisperConfig, WhisperEngine};
use crate::{Error, SttEngine};

/// Apple's `SpeechTranscriber` is a macOS framework; there is nothing to probe.
pub(crate) const APPLE_SPEECH_UNSUPPORTED: Option<&str> = Some(
    "it is part of macOS and does not exist on this system; set \"engine\" to \"auto\" or \"whisper\"",
);

/// Load the whisper model at `model` with the default config — what
/// [`crate::registry::select`] builds for the whisper choice.
pub(crate) fn load_whisper(model: &Path) -> Result<Box<dyn SttEngine>, Error> {
    Ok(Box::new(WhisperEngine::load(
        model,
        WhisperConfig::default(),
    )?))
}
