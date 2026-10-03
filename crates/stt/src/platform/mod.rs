//! The OS seam for speech-to-text (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. SPEC §2.5 keeps
//! engine *selection* in [`crate::registry`] and platform-free; what differs per
//! OS is only whether the whisper fallback is built at all, because
//! `whisper-rs` compiles whisper.cpp for the target and is a macOS-only
//! dependency for now (Cargo.toml). So:
//!
//! * macOS: `macos.rs` builds the real [`crate::whisper::WhisperEngine`].
//! * Windows: `windows.rs`, Linux (and any other OS): `linux.rs`. Explicit
//!   stubs that report the engine as unavailable until the port turns
//!   `whisper-rs` on there with `vulkan`/`cuda`.

// `whisper.rs` stays where it was; only its declaration moved here, so
// `lib.rs` has no `cfg` and `stt::whisper` keeps its public path on macOS.
#[cfg(target_os = "macos")]
#[path = "../whisper.rs"]
pub mod whisper;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod linux;

#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "windows")]
use windows as os;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use linux as os;

pub(crate) use os::load_whisper;

/// What `lib.rs` re-exports to other crates: `stt::whisper` on macOS, nothing
/// elsewhere.
pub mod public {
    #[cfg(target_os = "macos")]
    pub use super::whisper;
}
