//! ONNX Runtime on Windows, loaded at run time (TUR-62).
//!
//! `ort` is built with `load-dynamic` here (crates/stt/Cargo.toml), so
//! `onnxruntime.dll` must be loaded before the first ONNX call. It is loaded
//! by full path, from next to meet-ai.exe (or `ORT_DYLIB_PATH`): Windows 11
//! keeps an older `onnxruntime.dll` of its own in System32, and a bare-name
//! load can pick that one up and crash. Left to itself, `ort` would load by
//! bare name and panic if that failed, which is why this runs first.
//!
//! Shipping the DLL beside the app is the installer's job (a follow-up to the
//! release workflow); until then a missing DLL is a typed error that says so.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::Error;

/// The library `ort` 2.0.0-rc.13 loads on Windows.
const DLL: &str = "onnxruntime.dll";

/// ONNX Runtime is a separate file here, and may be missing.
#[cfg(test)]
pub(crate) const ONNX_RUNTIME_LOADED_AT_RUN_TIME: bool = true;

/// Set once the DLL is loaded. A failure is not remembered, so a DLL put in
/// place later is picked up on the next try.
static LOADED: OnceLock<()> = OnceLock::new();

/// Where the DLL should be: `ORT_DYLIB_PATH`, else next to the executable.
fn dll_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    crate::parakeet::onnx_runtime_path(std::env::var_os("ORT_DYLIB_PATH"), exe_dir.as_deref(), DLL)
}

/// Why the DLL cannot be used, or `None` when it is loaded or there to load.
/// A file check only; nothing is loaded.
pub(crate) fn onnx_runtime_missing() -> Option<String> {
    if LOADED.get().is_some() {
        return None;
    }
    crate::parakeet::onnx_runtime_problem(dll_path().as_deref(), DLL)
}

/// Load `onnxruntime.dll` once per process.
pub(crate) fn prepare_onnx_runtime() -> Result<(), Error> {
    if LOADED.get().is_some() {
        return Ok(());
    }
    if let Some(problem) = onnx_runtime_missing() {
        return Err(Error::EngineUnavailable(problem));
    }
    let path = dll_path().ok_or_else(|| {
        Error::EngineUnavailable(format!("the Parakeet engine could not find {DLL}"))
    })?;

    let environment = ort::init_from(&path).map_err(|error| {
        Error::EngineUnavailable(format!(
            "the Parakeet engine could not load ONNX Runtime from {}: {error}",
            path.display()
        ))
    })?;
    // `false` only means an environment was already committed, which is fine.
    let _ = environment.commit();
    let _ = LOADED.set(());
    tracing::info!(path = %path.display(), "loaded ONNX Runtime");
    Ok(())
}
