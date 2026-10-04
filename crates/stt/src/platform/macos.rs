//! The speech seam on macOS: Apple's engine can exist, and whisper is built
//! with Metal (Cargo.toml).

/// `None`: macOS 26+ has Apple's engine; the registry probes the sidecar.
pub(crate) const APPLE_SPEECH_UNSUPPORTED: Option<&str> = None;
