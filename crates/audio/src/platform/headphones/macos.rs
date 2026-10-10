//! The default output on macOS (TUR-65): Core Audio's transport type, the
//! data source (built-in speakers or the headphone jack) and the output
//! streams' terminal types. Property reads on the default output device
//! only.
//! The reads themselves are `macos/props.rs`'s (TUR-177).

use objc2_core_audio::{self as ca, AudioObjectID};

use super::parse;
use crate::Error;
use crate::headphones::{OutputDevice, Transport};
use crate::platform::macos::props;

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

/// One fixed-size property, or `None` when the object does not have it.
///
/// # Safety
/// `selector` must name a property whose value is a `T`.
unsafe fn read<T: Copy>(id: AudioObjectID, selector: u32, scope: u32) -> Option<T> {
    unsafe { props::read(id, selector, scope) }.ok()
}

/// The device's name, a `CFString` Core Audio hands over with +1 retain.
fn name(id: AudioObjectID) -> Option<String> {
    let scope = ca::kAudioObjectPropertyScopeGlobal;
    props::read_cf_string(id, ca::kAudioObjectPropertyName, scope).ok()
}

/// The device's output streams; empty when it has none or the read fails.
fn output_streams(id: AudioObjectID) -> Vec<AudioObjectID> {
    let (selector, scope) = (
        ca::kAudioDevicePropertyStreams,
        ca::kAudioObjectPropertyScopeOutput,
    );
    props::read_array(id, selector, scope).unwrap_or_default()
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
