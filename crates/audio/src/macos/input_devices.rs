//! The input devices Core Audio lists, for [`crate::mic_choice::choose`]
//! (TUR-91): each device's UID, name, transport (built-in, Bluetooth, other)
//! and whether it is the default input. Property reads only: nothing here
//! opens a stream, so nothing here switches a headset to HFP.

use std::ptr::NonNull;

use objc2_core_audio::{self as ca, AudioObjectID};
use objc2_core_foundation::{CFRetained, CFString};

use super::device_watch::default_input_device;
use super::tap_rate::{input_streams, read};
use crate::Error;
use crate::mic_choice::{InputDevice, Transport};

/// Core Audio's transport type to the choice's [`Transport`].
pub(crate) fn transport_of(raw: u32) -> Transport {
    match raw {
        ca::kAudioDeviceTransportTypeBuiltIn => Transport::BuiltIn,
        ca::kAudioDeviceTransportTypeBluetooth | ca::kAudioDeviceTransportTypeBluetoothLE => {
            Transport::Bluetooth
        }
        _ => Transport::Other,
    }
}

/// Every device with at least one input stream.
pub(crate) fn input_devices() -> Result<Vec<InputDevice>, Error> {
    let default = default_input_device()?;
    Ok(all_devices()?
        .into_iter()
        .filter(|&id| input_streams(id).is_some_and(|s| !s.is_empty()))
        .filter_map(|id| {
            Some(InputDevice {
                uid: cf_string(id, ca::kAudioDevicePropertyDeviceUID)?,
                name: cf_string(id, ca::kAudioObjectPropertyName).unwrap_or_default(),
                transport: transport_of(global::<u32>(id, ca::kAudioDevicePropertyTransportType)?),
                is_default: id == default,
            })
        })
        .collect())
}

fn global<T: Copy>(id: AudioObjectID, selector: u32) -> Option<T> {
    // SAFETY: each caller names a selector whose value is a `T`.
    unsafe { read::<T>(id, selector, ca::kAudioObjectPropertyScopeGlobal) }
}

/// A `CFStringRef`-valued property, owned by the caller (Core Audio's "copy"
/// rule for these), wrapped so it is released on drop.
fn cf_string(id: AudioObjectID, selector: u32) -> Option<String> {
    let raw: *mut CFString = global(id, selector)?;
    // SAFETY: a non-null string Core Audio handed over with +1 retain.
    let owned: CFRetained<CFString> = unsafe { CFRetained::from_raw(NonNull::new(raw)?) };
    Some(owned.to_string())
}

/// `kAudioHardwarePropertyDevices` on the system object.
fn all_devices() -> Result<Vec<AudioObjectID>, Error> {
    let system = ca::kAudioObjectSystemObject as AudioObjectID;
    let mut addr = ca::AudioObjectPropertyAddress {
        mSelector: ca::kAudioHardwarePropertyDevices,
        mScope: ca::kAudioObjectPropertyScopeGlobal,
        mElement: ca::kAudioObjectPropertyElementMain,
    };
    let failed =
        |status| Error::NoDevice(format!("listing audio devices failed: OSStatus {status}"));
    let mut size = 0u32;
    // SAFETY: valid address and out-pointer for the duration of the call.
    let status = unsafe {
        ca::AudioObjectGetPropertyDataSize(
            system,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return Err(failed(status));
    }
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    let mut ids = vec![ca::kAudioObjectUnknown; count];
    if count == 0 {
        return Ok(ids);
    }
    // SAFETY: `ids` holds `size` bytes of `AudioObjectID`s.
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            system,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut ids[0]).cast(),
        )
    };
    if status != 0 {
        return Err(failed(status));
    }
    ids.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_classic_and_le_both_count_as_bluetooth() {
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeBluetooth),
            Transport::Bluetooth
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeBluetoothLE),
            Transport::Bluetooth
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeBuiltIn),
            Transport::BuiltIn
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeUSB),
            Transport::Other
        );
    }

    /// Like `device_watch`'s smoke test: reads properties of whatever this
    /// machine has, opens nothing.
    #[test]
    fn the_device_list_is_readable_on_this_machine() {
        let devices = input_devices();
        assert!(devices.is_ok(), "{devices:?}");
    }
}
