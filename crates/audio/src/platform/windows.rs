//! The audio seam on Windows.
//!
//! Real here: system audio as WASAPI loopback on the default output device
//! (`windows_loopback.rs`), the QPC host clock both tracks' positions use and
//! the default-device watch behind the headset-switch reopen
//! (`windows_devices.rs`, TUR-37), the device-activity read over WASAPI audio
//! sessions (`windows/activity.rs`, TUR-60), the permission checks
//! (`windows_permission.rs`, TUR-51), and which render endpoint other apps
//! play to, so the loopback can follow a call on a non-default headset
//! (`windows/render_in_use.rs`, TUR-95).
//!
//! Still the shared non-mac answer from `other.rs`: the `cpal` microphone
//! (real, and the same code as on macOS) and the plain start sound. Nothing
//! outside `platform/` has to change when those are filled in.

mod activity;
// TUR-65: the default output's form factor and bus, for the headphone warning.
mod headphones;
// TUR-95: the render endpoint other apps are playing to.
mod render_in_use;

#[cfg(test)]
pub(crate) use super::other::F32_GOLDEN_HASHES;
pub(crate) use super::other::input_devices;
pub(crate) use super::other::{mic_source, start_sound};
// TUR-37: system audio, the host clock and the device watch.
pub(crate) use super::windows_devices::{
    DeviceId, default_input_device, default_output_device, host_now_ns, input_callback_ns,
};
pub(crate) use super::windows_loopback::system_source;
// TUR-51: this OS's own permission checks.
pub(crate) use super::windows_permission::{check_mic, check_system, stored_mic_denial};
pub(crate) use activity::device_activity;
#[cfg(test)]
pub(crate) use activity::{DEVICE_ACTIVITY, DEVICE_ACTIVITY_NEEDS_SERVER};
pub(crate) use headphones::default_output_info;
pub(crate) use render_in_use::render_endpoint_to_follow;
