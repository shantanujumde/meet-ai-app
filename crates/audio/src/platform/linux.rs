//! The audio seam on Linux.
//!
//! Real here: system audio as an input stream on the default sink through
//! `cpal`'s PipeWire host, or the default sink's monitor on PulseAudio
//! (`linux/loopback.rs`), the CLOCK_MONOTONIC host clock both tracks'
//! positions use (`linux/clock.rs`), the default sink and source watch
//! behind the headset-switch reopen (`linux/devices.rs`, all TUR-38), the
//! device-activity read over the sound server's streams (`linux/activity.rs`,
//! TUR-60), and the permission checks (`linux_permission.rs`, TUR-51).
//!
//! Still the shared non-mac answer from `other.rs`: the `cpal` microphone
//! (real, the same code as on macOS, which refuses ALSA's `null` device) and
//! the plain start sound. Nothing outside `platform/` has to change when
//! those are filled in.

mod activity;
mod clock;
mod devices;
// TUR-65: the default sink's active port, for the headphone warning.
mod headphones;
mod loopback;

#[cfg(test)]
pub(crate) use super::other::F32_GOLDEN_HASHES;
pub(crate) use super::other::input_devices;
pub(crate) use super::other::{SILENT_SYSTEM_DENIAL, mic_source, start_sound};
// TUR-51: this OS's own permission checks.
pub(crate) use super::linux_permission::{check_mic, check_system, stored_mic_denial};
pub(crate) use activity::device_activity;
pub(crate) use activity::mic_users;
#[cfg(test)]
pub(crate) use activity::{DEVICE_ACTIVITY, DEVICE_ACTIVITY_NEEDS_SERVER};
// TUR-38: system audio, the host clock and the device watch.
pub(crate) use clock::{host_now_ns, input_callback_ns};
pub(crate) use devices::{DeviceId, default_input_device, default_output_device};
pub(crate) use headphones::default_output_info;
pub(crate) use loopback::system_source;
