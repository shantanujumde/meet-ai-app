//! The speech seam on macOS: Apple's engine can exist, and whisper is built
//! with Metal.

use std::path::Path;

use super::whisper::{WhisperConfig, WhisperEngine};
use crate::{Error, SttEngine};

/// `None`: macOS 26+ has Apple's engine; the registry probes the sidecar.
pub(crate) const APPLE_SPEECH_UNSUPPORTED: Option<&str> = None;

/// Load the whisper model at `model` with the default config — what
/// [`crate::registry::select`] builds for the whisper choice.
pub(crate) fn load_whisper(model: &Path) -> Result<Box<dyn SttEngine>, Error> {
    Ok(Box::new(WhisperEngine::load(
        model,
        WhisperConfig::default(),
    )?))
}
