//! Is anything using the default mic or speakers right now? (TUR-31)
//!
//! The audio-activity meeting signal (`docs/problem.md` item 42) needs only
//! two yes/no answers: is some process running I/O on the default input
//! device, and on the default output device. Core Audio keeps exactly that as
//! `kAudioDevicePropertyDeviceIsRunningSomewhere`, a plain property of the
//! device. Reading it opens no stream, installs no tap and captures nothing,
//! so it needs no TCC permission and cannot raise a prompt — unlike
//! [`super::tap`] or [`crate::mic`].
//!
//! Each [`read`] is four property reads (two default-device ids, two
//! running flags), no allocation. The detection loop calls it every 2 s.
//!
//! The answer does not say *who* is running the device: while meet-ai itself
//! records, its own mic stream makes the input "running". The caller ignores
//! readings taken while recording (`detect::activity`).

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};

use super::device_watch::{default_input_device, default_output_device};
use crate::Error;

/// One reading of the default devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceActivity {
    /// Some process is running I/O on the default input device (a mic).
    pub input_running: bool,
    /// Some process is running I/O on the default output device.
    pub output_running: bool,
}

/// Read both flags for the current default devices. The default device ids
/// are re-read every time, so plugging in a headset is followed at once.
pub fn read() -> Result<DeviceActivity, Error> {
    Ok(DeviceActivity {
        input_running: is_running_somewhere(default_input_device()?)?,
        output_running: is_running_somewhere(default_output_device()?)?,
    })
}

/// `kAudioDevicePropertyDeviceIsRunningSomewhere` of `device`. A missing
/// device (`kAudioObjectUnknown`, e.g. a Mac mini with no mic) is not running.
fn is_running_somewhere(device: AudioObjectID) -> Result<bool, Error> {
    if device == ca::kAudioObjectUnknown {
        return Ok(false);
    }
    let mut address = AudioObjectPropertyAddress {
        mSelector: ca::kAudioDevicePropertyDeviceIsRunningSomewhere,
        mScope: ca::kAudioObjectPropertyScopeGlobal,
        mElement: ca::kAudioObjectPropertyElementMain,
    };
    let mut size = std::mem::size_of::<u32>() as u32;
    let mut value: u32 = 0;
    // SAFETY: the property is a scalar `UInt32` of an audio device; `size`
    // and `value` describe a `u32` that outlives the call, and no qualifier
    // is passed (size 0, null pointer).
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            device,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
            std::ptr::NonNull::from(&mut value).cast(),
        )
    };
    if status != 0 {
        return Err(Error::NoDevice(format!(
            "AudioObjectGetPropertyData(DeviceIsRunningSomewhere) on device {device} failed: \
             OSStatus {status}"
        )));
    }
    Ok(value != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_device_is_not_running() {
        assert_eq!(
            is_running_somewhere(ca::kAudioObjectUnknown).ok(),
            Some(false)
        );
    }

    /// Like `device_watch`'s smoke test: whatever this Mac is doing, the read
    /// itself succeeds. It reads properties only — no stream, no permission
    /// prompt.
    #[test]
    fn the_default_devices_activity_is_readable_on_this_machine() {
        let reading = read();
        assert!(reading.is_ok(), "{reading:?}");
    }
}
