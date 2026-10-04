//! ONNX Runtime where it is linked into the binary: macOS and Linux (TUR-62).
//!
//! ort-sys's build script fetched pyke's prebuilt ONNX Runtime and linked it
//! statically (`download-binaries`, crates/stt/Cargo.toml), so there is nothing
//! to find or load before the first ONNX call.

/// ONNX Runtime is part of this binary, so it cannot be missing.
#[cfg(test)]
pub(crate) const ONNX_RUNTIME_LOADED_AT_RUN_TIME: bool = false;

/// Never missing: it is linked in.
pub(crate) fn onnx_runtime_missing() -> Option<String> {
    None
}

/// Nothing to do: the library is already linked in.
pub(crate) fn prepare_onnx_runtime() -> Result<(), crate::Error> {
    Ok(())
}
