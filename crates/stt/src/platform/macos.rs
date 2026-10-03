//! The speech seam on macOS: the whisper fallback is built (Metal).

use std::path::Path;

use super::whisper::{WhisperConfig, WhisperEngine};
use crate::{Error, SttEngine};

/// Load the whisper model at `model` with the default config — what
/// [`crate::registry::select`] builds for the whisper choice.
pub(crate) fn load_whisper(model: &Path) -> Result<Box<dyn SttEngine>, Error> {
    Ok(Box::new(WhisperEngine::load(
        model,
        WhisperConfig::default(),
    )?))
}
