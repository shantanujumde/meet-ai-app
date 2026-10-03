//! The speech seam on Windows: an explicit stub until the Windows parity ticket.
//!
//! `whisper-rs` is not a dependency here yet (Cargo.toml gates it to macOS),
//! so there is no whisper engine to construct. The port turns it on with the
//! `vulkan`/`cuda` features and replaces this with the real load.

use std::path::Path;

use crate::{Error, SttEngine};

/// Always [`Error::EngineUnavailable`]: whisper is not built for Windows yet.
pub(crate) fn load_whisper(_model: &Path) -> Result<Box<dyn SttEngine>, Error> {
    Err(Error::EngineUnavailable(
        "the whisper engine is not built for this platform yet".into(),
    ))
}
