//! Default-device-change detection (SPEC §5's AirPods connect/disconnect
//! exit-gate condition).
//!
//! Core Audio can push a change through `AudioObjectAddPropertyListenerBlock`
//! instead of being polled, but that callback fires on an arbitrary Core
//! Audio thread and would need to hand its result back into `meet-rec`'s
//! orchestrator — which owns the segment-reopen state — through some other
//! channel anyway. Polling the same property from the orchestrator's own
//! loop is simpler, has no callback-lifetime question, and is fast enough:
//! `meet-rec`'s poll cadence (200 ms) is well under
//! [`crate::segments::CLOSE_ANCHOR_SLACK_MS`] (250 ms), the budget the
//! on-disk format already allows a segment close to be late by.

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};

use crate::Error;

/// # Safety
/// `selector` must be a scalar `AudioObjectID`-valued property of
/// `kAudioObjectSystemObject` — true of both properties this module reads.
unsafe fn system_object_device_id(selector: u32) -> Result<AudioObjectID, Error> {
    let mut address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: ca::kAudioObjectPropertyScopeGlobal,
        mElement: ca::kAudioObjectPropertyElementMain,
    };
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;
    let mut value: AudioObjectID = ca::kAudioObjectUnknown;
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            ca::kAudioObjectSystemObject as AudioObjectID,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
            std::ptr::NonNull::from(&mut value).cast(),
        )
    };
    if status != 0 {
        return Err(Error::NoDevice(format!(
            "AudioObjectGetPropertyData(0x{selector:08x}) on the system object failed: \
             OSStatus {status}"
        )));
    }
    Ok(value)
}

/// The current default output device id — what `SystemSource`'s tap rides
/// alongside. Plugging in or removing AirPods changes this, which is the
/// scenario `SystemSource` cannot itself keep recording through: the tap's
/// private aggregate device is built against a specific output device, and
/// following a swap means tearing down and rebuilding it (contract §5/§11's
/// `reason::DEFAULT_OUTPUT_DEVICE_CHANGED`).
pub fn default_output_device() -> Result<AudioObjectID, Error> {
    // SAFETY: `kAudioHardwarePropertyDefaultOutputDevice` is a scalar
    // `AudioObjectID` property of the system object.
    unsafe { system_object_device_id(ca::kAudioHardwarePropertyDefaultOutputDevice) }
}

/// The current default input device id — what `cpal`'s
/// `default_input_device()` resolves to under the hood. AirPods can also
/// become the default *input* once connected, which `MicSource`'s open
/// `cpal` stream does not itself follow
/// (`reason::DEFAULT_INPUT_DEVICE_CHANGED`).
pub fn default_input_device() -> Result<AudioObjectID, Error> {
    // SAFETY: `kAudioHardwarePropertyDefaultInputDevice` is a scalar
    // `AudioObjectID` property of the system object.
    unsafe { system_object_device_id(ca::kAudioHardwarePropertyDefaultInputDevice) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not hardware-independent — this machine's actual default devices are
    /// whatever is plugged in — but a call that returns *some* device id
    /// rather than an error is itself a useful smoke check that the property
    /// selector and object id are wired correctly, and it costs nothing
    /// (reads a property, no permission prompt, no capture).
    #[test]
    fn default_devices_are_readable_on_this_machine() {
        let output = default_output_device();
        let input = default_input_device();
        assert!(output.is_ok(), "default output device: {output:?}");
        assert!(input.is_ok(), "default input device: {input:?}");
    }
}
