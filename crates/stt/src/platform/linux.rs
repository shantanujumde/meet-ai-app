//! The speech seam on Linux: an explicit stub until the Linux parity ticket. Also compiled for any OS other than macOS and Windows.
//!
//! `whisper-rs` is not a dependency here yet (Cargo.toml gates it to macOS),
//! so there is no whisper engine to construct. The port turns it on with the
//! `vulkan`/`cuda` features and replaces this with the real load.

use std::path::Path;

use crate::{Error, SttEngine};

/// Always [`Error::EngineUnavailable`]: whisper is not built for Linux yet.
pub(crate) fn load_whisper(_model: &Path) -> Result<Box<dyn SttEngine>, Error> {
    Err(Error::EngineUnavailable(
        "the whisper engine is not built for this platform yet".into(),
    ))
}
