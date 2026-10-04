//! The audio seam on an OS without its own implementation yet.
//!
//! What `windows.rs` and `linux.rs` start from, and what any other OS gets
//! directly. `cpal` already runs everywhere, so the microphone is real; the
//! rest is the "absent" answer each caller already handles: no system-audio
//! source (contract §9's absent track), no device watch, and
//! [`Error::Unsupported`] from the activity read.

use crate::activity::DeviceActivity;
use crate::permission_check::{self, ChannelResult, ChannelState};
use crate::{AudioSource, Error};

/// A default-device id. Never produced here: [`default_output_device`] and
/// [`default_input_device`] always fail.
pub(crate) type DeviceId = u32;

/// [`device_activity`] has no reading to give here.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY: bool = false;

/// The resampler's f32 golden hashes (`resample.rs` tests) were taken on
/// macOS. Another libm rounds `sin` differently, so the synthetic input and
/// the hash differ there; TUR-50 makes those tests portable.
#[cfg(test)]
pub(crate) const F32_GOLDEN_HASHES: bool = false;

/// No default-device watch yet, so a recording never reopens its segment on a
/// device change.
pub(crate) fn default_output_device() -> Result<DeviceId, Error> {
    Err(Error::Unsupported)
}

/// See [`default_output_device`].
pub(crate) fn default_input_device() -> Result<DeviceId, Error> {
    Err(Error::Unsupported)
}

/// No input-device list yet, so the microphone stays the default input
/// (TUR-91's Bluetooth choice is macOS only).
pub(crate) fn input_devices() -> Result<Vec<crate::mic_choice::InputDevice>, Error> {
    Err(Error::Unsupported)
}

/// SPEC §8.2's Windows port is a stub, not held to the drift gate, so a
/// process-relative monotonic clock is sufficient here — there is no second
/// channel on that platform yet for it to be compared against.
pub(crate) fn host_now_ns() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// No system-audio capture yet: the recording is microphone-only.
pub(crate) fn system_source() -> Option<Box<dyn AudioSource>> {
    None
}

/// The `cpal` microphone.
pub(crate) fn mic_source() -> Box<dyn AudioSource> {
    Box::new(crate::mic::MicSource::new())
}

/// No device-activity read yet.
pub(crate) fn device_activity() -> Result<DeviceActivity, Error> {
    Err(Error::Unsupported)
}

/// No OS-level microphone decision to read without opening a stream.
pub(crate) fn stored_mic_denial() -> Option<ChannelResult> {
    None
}

/// Open the `cpal` microphone briefly.
pub(crate) fn check_mic() -> ChannelResult {
    permission_check::check_mic_with(mic_source())
}

/// No system-audio capture to check yet.
pub(crate) fn check_system() -> ChannelResult {
    ChannelResult {
        state: ChannelState::Unmeasurable,
        detail: "system-audio capture is not implemented on this platform yet".into(),
    }
}
