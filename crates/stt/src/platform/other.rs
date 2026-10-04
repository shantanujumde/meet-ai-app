//! The speech seam on Windows, Linux and any OS other than macOS: whisper is
//! the only engine.
//!
//! whisper.cpp is compiled with `GGML_NATIVE=OFF` (cmake/force-portable-ggml.cmake)
//! so the binary is not tuned to the build machine's CPU. Windows x64 and
//! Linux also get ggml's Vulkan backend (TUR-61, Cargo.toml); Windows on ARM
//! and anything else stay CPU only. With no Vulkan driver ggml registers no
//! GPU and whisper runs on the CPU.

/// Apple's `SpeechTranscriber` is a macOS framework; there is nothing to probe.
pub(crate) const APPLE_SPEECH_UNSUPPORTED: Option<&str> = Some(
    "it is part of macOS and does not exist on this system; set \"engine\" to \"auto\" or \"whisper\"",
);

/// How a recommendation names this machine.
pub(crate) const MACHINE: &str = if cfg!(windows) {
    "This PC"
} else {
    "This computer"
};
/// "{MACHINE} has {GPU_YES}".
pub(crate) const GPU_YES: &str = "a graphics chip meet-ai can use";
/// "{MACHINE} has {GPU_NONE}".
pub(crate) const GPU_NONE: &str = "no graphics chip meet-ai can use";

/// Ask ggml which GPUs it registered: Vulkan devices on a Vulkan build, none
/// on a CPU-only one. This starts the Vulkan loader in-process, as loading a
/// model does; a broken driver that crashes there is not covered by the GPU
/// crash marker (see `crate::gpu_guard`).
pub(crate) fn gpu_present() -> bool {
    let devices = crate::hardware::ggml_gpu_devices();
    tracing::info!(?devices, "GPUs whisper can use");
    !devices.is_empty()
}
