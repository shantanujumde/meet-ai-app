//! The OS seam for audio capture (SPEC §8.2).
//!
//! This is the only file in the crate that names an operating system. Each OS
//! gets one module with the same set of functions, and the rest of the crate
//! calls `crate::platform::…` without a `cfg` of its own:
//!
//! * macOS: [`crate::macos`] (the directory SPEC §8.2 names), seam functions in
//!   `macos/platform.rs`.
//! * Windows: `windows.rs`, Linux: `linux.rs`. Explicit stubs today (no
//!   system-audio tap, no device watch, no activity read); the parity tickets
//!   fill them in. Both start from the shared non-mac code in `other.rs`.
//! * Any other OS: `other.rs` directly.
//!
//! The `stub-audio` feature swaps the capture half (sources and permission
//! checks) for no-op versions in `stub.rs`, on any OS, so CI can build and test
//! the crate without audio devices. The host clock and the device reads stay
//! the real OS ones. Off by default; the app never turns it on.
//!
//! Adding a port means filling in that OS's file. Nothing outside this folder
//! and the per-OS folders should need to change.

// `#[path]` keeps the macOS code where SPEC §8.2 says it lives
// (`src/macos/`) while routing it through here, so `lib.rs` has no `cfg`.
#[cfg(target_os = "macos")]
#[cfg_attr(feature = "stub-audio", allow(dead_code))]
#[path = "../macos/mod.rs"]
pub mod macos;

#[cfg(target_os = "windows")]
#[cfg_attr(feature = "stub-audio", allow(dead_code, unused_imports))]
mod windows;

#[cfg(target_os = "linux")]
#[cfg_attr(feature = "stub-audio", allow(dead_code, unused_imports))]
mod linux;

#[cfg(not(target_os = "macos"))]
#[cfg_attr(feature = "stub-audio", allow(dead_code))]
mod other;

// TUR-51: the Windows consent rules are pure, so they build (and are tested)
// on every OS; only `windows_permission` reads the registry.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod windows_consent;

#[cfg(target_os = "windows")]
#[cfg_attr(feature = "stub-audio", allow(dead_code, unused_imports))]
mod windows_permission;

#[cfg(target_os = "linux")]
#[cfg_attr(feature = "stub-audio", allow(dead_code, unused_imports))]
mod linux_permission;

// TUR-37: WASAPI loopback, the QPC clock and the default-device watch.
#[cfg(target_os = "windows")]
#[cfg_attr(feature = "stub-audio", allow(dead_code, unused_imports))]
mod windows_devices;

#[cfg(target_os = "windows")]
#[cfg_attr(feature = "stub-audio", allow(dead_code, unused_imports))]
mod windows_loopback;

// TUR-95: which render endpoint the loopback follows. Pure, so it builds (and
// is tested) on every OS; `windows/render_in_use.rs` reads the sessions.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod windows_render_choice;

#[cfg(feature = "stub-audio")]
mod stub;

// TUR-65: what the default output is, for the headphone warning. Always the
// real OS read, under `stub-audio` too.
mod headphones;
pub(crate) use headphones::default_output_info;

/// What `lib.rs` re-exports to other crates: `audio::macos` on macOS, nothing
/// elsewhere.
pub mod public {
    #[cfg(target_os = "macos")]
    pub use super::macos;
}

#[cfg(target_os = "macos")]
use macos::platform as os;

#[cfg(target_os = "windows")]
use windows as os;

#[cfg(target_os = "linux")]
use linux as os;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use other as os;

// Always the real OS: the host clock every anchor uses, the default-device
// watch behind the AirPods-swap reopen, and the device-activity read.
#[cfg(test)]
pub(crate) use os::DEVICE_ACTIVITY;
#[cfg(test)]
pub(crate) use os::DEVICE_ACTIVITY_NEEDS_SERVER;
#[cfg(test)]
pub(crate) use os::F32_GOLDEN_HASHES;
pub(crate) use os::input_devices;
// TUR-142: which apps are using a mic.
pub(crate) use os::mic_users;
pub(crate) use os::{
    DeviceId, default_input_device, default_output_device, device_activity, host_now_ns,
};
// The capture time a microphone callback carries, where it shares the host clock (TUR-37).
pub(crate) use os::input_callback_ns;

// Capture and permission checks: the real OS, or no-ops under `stub-audio`.
#[cfg(not(feature = "stub-audio"))]
pub(crate) use os::SILENT_SYSTEM_DENIAL;
#[cfg(not(feature = "stub-audio"))]
pub(crate) use os::start_sound;
#[cfg(not(feature = "stub-audio"))]
pub(crate) use os::{check_mic, check_system, mic_source, stored_mic_denial, system_source};

#[cfg(feature = "stub-audio")]
pub(crate) use stub::SILENT_SYSTEM_DENIAL;
#[cfg(feature = "stub-audio")]
pub(crate) use stub::start_sound;
#[cfg(feature = "stub-audio")]
pub(crate) use stub::{check_mic, check_system, mic_source, stored_mic_denial, system_source};
