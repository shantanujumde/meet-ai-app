//! The default output on macOS (TUR-65): Core Audio's transport type, the
//! data source (built-in speakers or the headphone jack) and the output
//! streams' terminal types. Property reads on the default output device
//! only.
//!
//! `macos/` has its own property helpers, but that folder is another
//! ticket's; these few reads stay here.

use std::ptr::NonNull;

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
use objc2_core_foundation::{CFRetained, CFString};

use super::parse;
use crate::Error;
use crate::headphones::{OutputDevice, Transport};

/// The default output device, or `None` when there is none.
pub(crate) fn default_output_info() -> Result<Option<OutputDevice>, Error> {
    let id = crate::platform::default_output_device()?;
    if id == ca::kAudioObjectUnknown {
        return Ok(None);
    }
    let output = ca::kAudioObjectPropertyScopeOutput;
    let global = ca::kAudioObjectPropertyScopeGlobal;
    // SAFETY: each selector below names a property whose value is the type read.
    let transport = unsafe { read::<u32>(id, ca::kAudioDevicePropertyTransportType, global) };
    let data_source = unsafe { read::<u32>(id, ca::kAudioDevicePropertyDataSource, output) };
    let terminal_types: Vec<u32> = output_streams(id)
        .into_iter()
        .filter_map(|stream| unsafe {
            read::<u32>(stream, ca::kAudioStreamPropertyTerminalType, global)
        })
        .collect();
    Ok(Some(OutputDevice {
        name: name(id).unwrap_or_default(),
        transport: transport.map_or(Transport::Other, transport_of),
        form: parse::mac_form(data_source, &terminal_types),
    }))
}

/// Core Audio's transport type to [`Transport`].
fn transport_of(raw: u32) -> Transport {
    match raw {
        ca::kAudioDeviceTransportTypeBuiltIn => Transport::BuiltIn,
        ca::kAudioDeviceTransportTypeBluetooth | ca::kAudioDeviceTransportTypeBluetoothLE => {
            Transport::Bluetooth
        }
        ca::kAudioDeviceTransportTypeUSB => Transport::Usb,
        ca::kAudioDeviceTransportTypeHDMI | ca::kAudioDeviceTransportTypeDisplayPort => {
            Transport::Display
        }
        ca::kAudioDeviceTransportTypeAirPlay => Transport::AirPlay,
        ca::kAudioDeviceTransportTypeVirtual | ca::kAudioDeviceTransportTypeAggregate => {
            Transport::Virtual
        }
        _ => Transport::Other,
    }
}

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: ca::kAudioObjectPropertyElementMain,
    }
}

/// One fixed-size property, or `None` when the object does not have it.
///
/// # Safety
/// `selector` must name a property whose value is a `T`.
unsafe fn read<T: Copy>(id: AudioObjectID, selector: u32, scope: u32) -> Option<T> {
    let mut addr = address(selector, scope);
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    // SAFETY: valid address, size and out-pointer for the duration of the call.
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(value.as_mut_ptr().cast())?,
        )
    };
    // SAFETY: `noErr` with the full size written means Core Audio filled a `T`.
    (status == 0 && size as usize == std::mem::size_of::<T>())
        .then(|| unsafe { value.assume_init() })
}

/// The device's name, a `CFString` Core Audio hands over with +1 retain.
fn name(id: AudioObjectID) -> Option<String> {
    // SAFETY: `kAudioObjectPropertyName` is a `CFStringRef`.
    let raw: *mut CFString = unsafe {
        read(
            id,
            ca::kAudioObjectPropertyName,
            ca::kAudioObjectPropertyScopeGlobal,
        )
    }?;
    // SAFETY: a non-null string Core Audio handed over with +1 retain.
    let owned: CFRetained<CFString> = unsafe { CFRetained::from_raw(NonNull::new(raw)?) };
    Some(owned.to_string())
}

/// The device's output streams; empty when it has none or the read fails.
fn output_streams(id: AudioObjectID) -> Vec<AudioObjectID> {
    let mut addr = address(
        ca::kAudioDevicePropertyStreams,
        ca::kAudioObjectPropertyScopeOutput,
    );
    let mut size = 0u32;
    // SAFETY: valid address and out-pointer for the duration of the call.
    let status = unsafe {
        ca::AudioObjectGetPropertyDataSize(
            id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    if status != 0 || count == 0 {
        return Vec::new();
    }
    let mut streams = vec![ca::kAudioObjectUnknown; count];
    let mut size = (count * std::mem::size_of::<AudioObjectID>()) as u32;
    // SAFETY: `streams` holds `size` bytes of `AudioStreamID`s.
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut streams[0]).cast(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    streams.truncate(size as usize / std::mem::size_of::<AudioObjectID>());
    streams
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminal_type_constant_matches_core_audio() {
        assert_eq!(
            parse::TERMINAL_HEADPHONES,
            ca::kAudioStreamTerminalTypeHeadphones
        );
    }

    #[test]
    fn transports_map() {
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeBuiltIn),
            Transport::BuiltIn
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeBluetoothLE),
            Transport::Bluetooth
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeDisplayPort),
            Transport::Display
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeAirPlay),
            Transport::AirPlay
        );
        assert_eq!(
            transport_of(ca::kAudioDeviceTransportTypeAggregate),
            Transport::Virtual
        );
        assert_eq!(transport_of(0), Transport::Other);
    }

    /// Like `device_watch`'s smoke test: reads properties of whatever this
    /// machine has, opens nothing. What it finds depends on the machine.
    #[test]
    fn the_default_output_is_readable_on_this_machine() {
        let read = default_output_info();
        assert!(read.is_ok(), "{read:?}");
        // `cargo test -p audio default_output -- --nocapture` shows what this
        // Mac's output reads as, for the manual checks.
        if let Ok(device) = &read {
            let kind = crate::headphones::classify(device.as_ref());
            eprintln!("default output: {device:?} -> {kind:?}");
        }
    }
}
