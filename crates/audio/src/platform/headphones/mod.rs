//! The default output device, read for the headphone warning (TUR-65).
//!
//! [`parse`] maps each OS's raw answers and is pure, so it is tested on
//! every OS. The reads: `macos.rs` here (the `macos/` folder is another
//! ticket's), `windows/headphones.rs` and `linux/headphones.rs` beside the
//! COM and PulseAudio helpers they reuse. Property reads only, everywhere:
//! nothing opens a stream, so nothing prompts for a permission or changes
//! what the user hears.

// Each OS calls only its own part; all of it is tested on every OS.
#[allow(dead_code)]
pub(super) mod parse;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::default_output_info;

#[cfg(target_os = "windows")]
pub(crate) use super::windows::default_output_info;

#[cfg(target_os = "linux")]
pub(crate) use super::linux::default_output_info;

/// No read on other OSes: unknown, so no warning.
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) fn default_output_info() -> Result<Option<crate::headphones::OutputDevice>, crate::Error>
{
    Ok(None)
}
