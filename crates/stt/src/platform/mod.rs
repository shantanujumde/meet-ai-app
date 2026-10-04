//! The OS seam for speech-to-text (SPEC §8.2).
//!
//! The only file in the crate that names an operating system. SPEC §2.5 keeps
//! engine *selection* in [`crate::registry`] and platform-free; what differs per
//! OS is:
//!
//! * whether Apple's engine can exist at all ([`APPLE_SPEECH_UNSUPPORTED`]);
//! * how whisper is built: Metal on macOS (`macos.rs`), CPU on Windows
//!   (`windows.rs`) and on Linux and any other OS (`linux.rs`). GPU off macOS
//!   is TUR-61. The cargo features behind that are in Cargo.toml.

// `whisper.rs` stays where it was; only its declaration lives here, so
// `lib.rs` has no `cfg` and `stt::whisper` keeps its public path.
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

pub(crate) use os::{APPLE_SPEECH_UNSUPPORTED, load_whisper};

/// What `lib.rs` re-exports to other crates: `stt::whisper`.
pub mod public {
    pub use super::whisper;
}
