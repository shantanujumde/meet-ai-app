//! Each OS's raw answers to the [`Form`] and [`Transport`] that
//! [`crate::headphones::classify`] reads (TUR-65). Pure, so it builds and is
//! tested on every OS; only the reads in the sibling files are OS code.

use crate::headphones::{Form, Transport};

// --- macOS ------------------------------------------------------------------

/// A Core Audio four-char code as the `u32` its properties hold.
const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

/// `kAudioDevicePropertyDataSource` values on the built-in output, from
/// IOKit's `IOAudioTypes.h` (`kIOAudioOutputPortSubType*`).
pub(crate) const DATA_SOURCE_INTERNAL_SPEAKER: u32 = fourcc(b"ispk");
pub(crate) const DATA_SOURCE_EXTERNAL_SPEAKER: u32 = fourcc(b"espk");
pub(crate) const DATA_SOURCE_HEADPHONES: u32 = fourcc(b"hdpn");

/// `kAudioStreamTerminalTypeHeadphones` (`'hdph'`). The macOS file checks
/// it against objc2-core-audio's constant.
pub(crate) const TERMINAL_HEADPHONES: u32 = fourcc(b"hdph");

/// USB Audio terminal types Core Audio passes through for USB devices:
/// headphones (0x0302), a head-mounted display's audio (0x0303) and a
/// headset (0x0402), from the USB Audio Class "Terminal Types" table.
const USB_TERMINAL_HEADPHONES: u32 = 0x0302;
const USB_TERMINAL_HEAD_MOUNTED: u32 = 0x0303;
const USB_TERMINAL_HEADSET: u32 = 0x0402;

/// The form, from the output's data source and its output streams'
/// terminal types. The data source decides when there is one (the built-in
/// output switches it when headphones go into the jack). A speaker terminal
/// type is not trusted: whether macOS reports one for headphones is not
/// known, so only the headphone types count.
pub(crate) fn mac_form(data_source: Option<u32>, terminal_types: &[u32]) -> Form {
    match data_source {
        Some(DATA_SOURCE_HEADPHONES) => return Form::Headphones,
        Some(DATA_SOURCE_INTERNAL_SPEAKER | DATA_SOURCE_EXTERNAL_SPEAKER) => return Form::Speakers,
        _ => {}
    }
    let headphone_terminal = terminal_types.iter().any(|&t| {
        matches!(
            t,
            TERMINAL_HEADPHONES | USB_TERMINAL_HEADPHONES | USB_TERMINAL_HEAD_MOUNTED
        )
    });
    if headphone_terminal {
        return Form::Headphones;
    }
    if terminal_types.contains(&USB_TERMINAL_HEADSET) {
        return Form::Headset;
    }
    Form::Unknown
}

// --- Windows ----------------------------------------------------------------

/// `PKEY_AudioEndpoint_FormFactor` (the `EndpointFormFactor` enum). Line
/// level, S/PDIF and a display's audio are left unknown: headphones can sit
/// behind any of them.
pub(crate) fn windows_form(form_factor: Option<u32>) -> Form {
    match form_factor {
        // Speakers
        Some(1) => Form::Speakers,
        // Headphones
        Some(3) => Form::Headphones,
        // Headset, Handset
        Some(5 | 7) => Form::Headset,
        _ => Form::Unknown,
    }
}

/// The bus enumerator in an endpoint adapter's device path, such as `bthhfenum`
/// in `{2}.\\?\bthhfenum#bthhfpaudio#...`.
// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(crate) fn adapter_enumerator(device_id: &str) -> Option<&str> {
    let path = &device_id[device_id.find("\\\\?\\")? + 4..];
    path.split('#')
        .next()
        .filter(|enumerator| !enumerator.is_empty())
}

/// The transport, from the adapter's bus enumerator, else from the name.
/// Windows' in-box Bluetooth driver names its endpoints "Headset (X
/// Hands-Free AG Audio)" or "Headphones (X Stereo)" and never says
/// "Bluetooth", so the bus is the reliable signal. HFP (`BTHHFENUM`) and
/// A2DP/LE (`BTHENUM`, `BTHLEENUM`) all start with "bth".
// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(crate) fn windows_transport(enumerator: Option<&str>, name: &str) -> Transport {
    if let Some(enumerator) = enumerator {
        match enumerator.to_ascii_lowercase().as_str() {
            bus if bus.starts_with("bth") => return Transport::Bluetooth,
            "usb" => return Transport::Usb,
            "hdaudio" | "intelaudio" => return Transport::BuiltIn,
            "swd" | "root" => return Transport::Virtual,
            _ => {}
        }
    }
    let lower = name.to_lowercase();
    if lower.contains("bluetooth") || lower.contains("hands-free") {
        Transport::Bluetooth
    } else if lower.contains("usb") {
        Transport::Usb
    } else if lower.contains("hdmi") || lower.contains("displayport") {
        Transport::Display
    } else {
        Transport::Other
    }
}

// --- Linux ------------------------------------------------------------------

/// What the default sink says about itself, from PulseAudio (or
/// pipewire-pulse).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct PulseSink {
    /// The sink's name, such as `bluez_output.XX.1` or
    /// `alsa_output.pci-0000_00_1f.3.analog-stereo`.
    pub name: String,
    /// `device.description`, the name the desktop shows.
    pub description: Option<String>,
    /// The active port's name, such as `analog-output-headphones`.
    pub active_port: Option<String>,
    /// `device.bus`: `pci`, `usb`, `bluetooth`.
    pub bus: Option<String>,
    /// `device.form_factor`: `headphone`, `headset`, `speaker`, `internal`...
    pub form_factor: Option<String>,
    /// The Bluetooth profile: PulseAudio's `bluetooth.protocol`
    /// (`a2dp_sink`, `headset_head_unit`, `handsfree_head_unit`) or
    /// PipeWire's `api.bluez5.profile` (`a2dp-sink`, `headset-head-unit`).
    pub bluetooth_profile: Option<String>,
}

/// The form: the active port first (ALSA names jack ports after what is
/// plugged in, `analog-output-headphones` / `analog-output-speaker`, and
/// bluez names its ports `headset-output`, `headphone-output`,
/// `speaker-output`), then a headset Bluetooth profile, then
/// `device.form_factor`.
// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/linux.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(crate) fn pulse_form(sink: &PulseSink) -> Form {
    if let Some(port) = sink.active_port.as_deref().map(str::to_lowercase) {
        if port.contains("headset") || port.contains("hands-free") || port.contains("handsfree") {
            return Form::Headset;
        }
        if port.contains("headphone") {
            return Form::Headphones;
        }
        if port.contains("speaker") {
            return Form::Speakers;
        }
    }
    if let Some(profile) = sink.bluetooth_profile.as_deref().map(str::to_lowercase)
        && ["headset", "handsfree", "hands-free", "hsp", "hfp"]
            .iter()
            .any(|marker| profile.contains(marker))
    {
        return Form::Headset;
    }
    match sink.form_factor.as_deref() {
        Some("headphone") => Form::Headphones,
        Some("headset" | "handset" | "hands-free") => Form::Headset,
        Some("speaker") => Form::Speakers,
        _ => Form::Unknown,
    }
}

/// The transport, from `device.bus`, else the sink's name.
// Adapted from github.com/fastrepl/anarlog/crates/audio-device/src/linux.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
pub(crate) fn pulse_transport(sink: &PulseSink) -> Transport {
    match sink.bus.as_deref() {
        Some(bus) if bus.contains("bluetooth") => return Transport::Bluetooth,
        Some(bus) if bus.contains("usb") => return Transport::Usb,
        _ => {}
    }
    let name = sink.name.to_lowercase();
    if name.starts_with("bluez") {
        Transport::Bluetooth
    } else if name.contains("hdmi") || name.contains("displayport") {
        Transport::Display
    } else if name.contains("usb") {
        Transport::Usb
    } else {
        Transport::Other
    }
}

#[cfg(test)]
mod tests;
