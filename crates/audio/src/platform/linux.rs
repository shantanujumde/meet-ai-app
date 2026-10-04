//! The audio seam on Linux: an explicit stub until the Linux parity ticket.
//!
//! Every item is the shared non-mac answer from `other.rs`: a real `cpal`
//! microphone, no system-audio source, no default-device watch, no activity
//! read, and a process-relative host clock. The port replaces these one by one
//! with Linux code in this file (or a `linux/` folder): the system source with
//! a PipeWire/PulseAudio monitor source, the device watch with the sound server's default-sink change events. Nothing outside
//! `platform/` has to change when it does.

#[cfg(test)]
pub(crate) use super::other::DEVICE_ACTIVITY;
#[cfg(test)]
pub(crate) use super::other::F32_GOLDEN_HASHES;
pub(crate) use super::other::{
    DeviceId, default_input_device, default_output_device, device_activity, host_now_ns,
    mic_source, start_sound, system_source,
};
// TUR-51: this OS's own permission checks.
pub(crate) use super::linux_permission::{check_mic, check_system, stored_mic_denial};
