//! The OS seam for the app shell (SPEC §8.2).
//!
//! The one place in `src-tauri/src` where new OS-specific code goes, so the
//! rest of the shell carries no OS `cfg` (quality rule R10). Tauri's own
//! desktop-vs-mobile plugin gates in `lib.rs` and `notify.rs` are not OS
//! ports and stay where they are.
//!
//! Small enough today to be this one file. When a port needs more, it becomes
//! `macos.rs` / `windows.rs` / `linux.rs` beside it, the way the crates do it.

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use stt::SttEngine;

/// The whisper engine for `model`, the way `stt::registry::select` builds it
/// for the whisper choice. For the live-transcript end-to-end tests, which then
/// compile on every OS; whisper itself is only built for macOS so far.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn load_whisper(model: &Path) -> Result<Box<dyn SttEngine>, String> {
    let engine =
        stt::whisper::WhisperEngine::load(model, Default::default()).map_err(|e| e.to_string())?;
    Ok(Box::new(engine))
}

/// No whisper engine is built for this OS yet (`crates/stt/src/platform`).
#[cfg(all(test, not(target_os = "macos")))]
pub(crate) fn load_whisper(_model: &Path) -> Result<Box<dyn SttEngine>, String> {
    Err("the whisper engine is not built for this platform yet".into())
}
