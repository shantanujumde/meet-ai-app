//! The speech seam on Windows, Linux and any OS other than macOS: whisper is
//! the only engine, built for the CPU.
//!
//! whisper.cpp is compiled with `GGML_NATIVE=OFF` (cmake/force-portable-ggml.cmake)
//! so the binary is not tuned to the build machine's CPU. GPU
//! (`vulkan`/`cuda`) is TUR-61.

/// Apple's `SpeechTranscriber` is a macOS framework; there is nothing to probe.
pub(crate) const APPLE_SPEECH_UNSUPPORTED: Option<&str> = Some(
    "it is part of macOS and does not exist on this system; set \"engine\" to \"auto\" or \"whisper\"",
);
