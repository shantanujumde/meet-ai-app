//! The speech seam on macOS: Apple's engine can exist, and whisper is built
//! with Metal (Cargo.toml).

/// `None`: macOS 26+ has Apple's engine; the registry probes the sidecar.
pub(crate) const APPLE_SPEECH_UNSUPPORTED: Option<&str> = None;

/// How a recommendation names this machine.
pub(crate) const MACHINE: &str = "This Mac";
/// "{MACHINE} has {GPU_YES}": Metal on Apple silicon is the GPU whisper uses.
pub(crate) const GPU_YES: &str = "Apple silicon";
/// "{MACHINE} has {GPU_NONE}".
pub(crate) const GPU_NONE: &str = "no Apple silicon";

/// Apple silicon always has a Metal GPU; an Intel Mac's is not worth it for
/// whisper (TUR-79), and macOS 26 builds target Apple silicon anyway. Read from
/// the build target, so nothing is started to answer.
pub(crate) fn gpu_present() -> bool {
    cfg!(target_arch = "aarch64")
}
