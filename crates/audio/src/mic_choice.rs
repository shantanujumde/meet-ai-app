//! Which microphone a recording opens (TUR-91).
//!
//! Opening the microphone of Bluetooth headphones (AirPods, most buds) makes
//! macOS switch them from their playback profile (A2DP) to the headset
//! profile (HFP): what the user hears gets quieter, mono and duller for as
//! long as the mic is open, and the output runs at 16 kHz (the rates
//! `macos/tap_rate.rs` logs). Recording must never change what the user
//! hears, so when the default input is a Bluetooth device and the Mac has a
//! built-in mic, the built-in mic is recorded instead and the headphones stay
//! in playback mode. The `audio.use_builtin_mic_with_bluetooth` setting turns
//! this off (default on).
//!
//! [`choose`] is pure, so it is tested over fake device lists. The device
//! list itself is OS code, read through `crate::platform::input_devices`
//! (Core Audio in `macos/input_devices.rs`; no list elsewhere, so other OSes
//! keep the default input).

use std::sync::atomic::{AtomicBool, Ordering};

/// How an input device is connected, as far as the choice cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// The Mac's own microphone.
    BuiltIn,
    /// Bluetooth Classic or Bluetooth LE.
    Bluetooth,
    /// USB, an aggregate, a virtual device, anything else.
    Other,
}

/// One input device, as the OS lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    /// The OS's stable id for the device (Core Audio's device UID), which is
    /// what `cpal`'s `DeviceId` carries on macOS.
    pub uid: String,
    /// The name shown in System Settings, for the log.
    pub name: String,
    pub transport: Transport,
    /// Whether this is the system's default input right now.
    pub is_default: bool,
}

/// The outcome of [`choose`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicChoice {
    /// Open the system's default input, as before TUR-91.
    Default {
        /// Why, for the log.
        reason: &'static str,
    },
    /// Open this device instead of the default.
    Device {
        uid: String,
        name: String,
        /// Why, for the log.
        reason: &'static str,
    },
}

/// The microphone to record, given the OS's input devices and whether
/// `use_builtin_mic_with_bluetooth` is on.
///
/// Only a Bluetooth default input is ever replaced, and only by a built-in
/// mic. Everything else (a wired or USB headset, the built-in mic itself, no
/// built-in mic, the setting off) keeps the default input. With the default
/// kept on a Bluetooth headset, the headset is in HFP and its mic also shows
/// up in the tap's aggregate device; `macos/tap_buffers.rs` (TUR-87) still
/// keeps that mic out of `system.wav`.
pub fn choose(devices: &[InputDevice], use_builtin_with_bluetooth: bool) -> MicChoice {
    let Some(default) = devices.iter().find(|d| d.is_default) else {
        return MicChoice::Default {
            reason: "the default input is not in the device list",
        };
    };
    if default.transport != Transport::Bluetooth {
        return MicChoice::Default {
            reason: "the default input is not Bluetooth",
        };
    }
    if !use_builtin_with_bluetooth {
        return MicChoice::Default {
            reason: "the default input is Bluetooth, but the setting to use the Mac's own mic \
                     is off, so the headphones switch to headset mode",
        };
    }
    match devices.iter().find(|d| d.transport == Transport::BuiltIn) {
        Some(builtin) => MicChoice::Device {
            uid: builtin.uid.clone(),
            name: builtin.name.clone(),
            reason: "the default input is Bluetooth; recording the Mac's own mic keeps the \
                     headphones in playback mode",
        },
        None => MicChoice::Default {
            reason: "the default input is Bluetooth and this Mac has no built-in mic, so the \
                     headphones switch to headset mode",
        },
    }
}

/// `audio.use_builtin_mic_with_bluetooth`, as the app last set it. On until
/// told otherwise, matching the config default.
static USE_BUILTIN_WITH_BLUETOOTH: AtomicBool = AtomicBool::new(true);

/// Set what [`use_builtin_with_bluetooth`] returns. The app calls this from
/// `config.jsonc` before each recording and when the setting is saved, so
/// every microphone open (start, a device-change reopen, the permission
/// check) sees the same answer.
pub fn set_use_builtin_with_bluetooth(on: bool) {
    USE_BUILTIN_WITH_BLUETOOTH.store(on, Ordering::Relaxed);
}

/// See [`set_use_builtin_with_bluetooth`].
pub fn use_builtin_with_bluetooth() -> bool {
    USE_BUILTIN_WITH_BLUETOOTH.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(uid: &str, transport: Transport, is_default: bool) -> InputDevice {
        InputDevice {
            uid: uid.to_string(),
            name: format!("{uid} name"),
            transport,
            is_default,
        }
    }

    fn builtin() -> InputDevice {
        device("BuiltInMicrophoneDevice", Transport::BuiltIn, false)
    }

    #[test]
    fn a_bluetooth_default_input_records_the_builtin_mic() {
        let devices = [device("buds", Transport::Bluetooth, true), builtin()];
        assert!(matches!(
            choose(&devices, true),
            MicChoice::Device { ref uid, .. } if uid == "BuiltInMicrophoneDevice"
        ));
    }

    #[test]
    fn with_the_setting_off_the_bluetooth_mic_is_kept() {
        // The headset stays the mic, so TUR-87's tap-buffer handling (its mic
        // in the aggregate, kept out of system.wav) is what applies.
        let devices = [device("buds", Transport::Bluetooth, true), builtin()];
        assert!(matches!(choose(&devices, false), MicChoice::Default { .. }));
    }

    #[test]
    fn no_builtin_mic_keeps_the_bluetooth_default() {
        // A Mac mini or Studio: no mic of its own.
        let devices = [
            device("buds", Transport::Bluetooth, true),
            device("usb-interface", Transport::Other, false),
        ];
        assert!(matches!(
            choose(&devices, true),
            MicChoice::Default { reason } if reason.contains("no built-in mic")
        ));
    }

    #[test]
    fn a_default_that_is_not_bluetooth_is_kept() {
        for transport in [Transport::BuiltIn, Transport::Other] {
            let devices = [
                device("default", transport, true),
                builtin(),
                device("buds", Transport::Bluetooth, false),
            ];
            assert!(
                matches!(choose(&devices, true), MicChoice::Default { .. }),
                "{transport:?}"
            );
        }
    }

    #[test]
    fn an_empty_or_defaultless_list_keeps_the_default() {
        assert!(matches!(choose(&[], true), MicChoice::Default { .. }));
        let devices = [device("buds", Transport::Bluetooth, false), builtin()];
        assert!(matches!(choose(&devices, true), MicChoice::Default { .. }));
    }

    #[test]
    fn the_setting_is_on_until_turned_off() {
        assert!(use_builtin_with_bluetooth());
        set_use_builtin_with_bluetooth(false);
        assert!(!use_builtin_with_bluetooth());
        set_use_builtin_with_bluetooth(true);
        assert!(use_builtin_with_bluetooth());
    }
}
