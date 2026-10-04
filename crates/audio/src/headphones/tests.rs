//! [`classify`] over fake devices, on every OS (TUR-65).

use super::*;

fn device(name: &str, transport: Transport, form: Form) -> OutputDevice {
    OutputDevice {
        name: name.to_owned(),
        transport,
        form,
    }
}

fn kind(name: &str, transport: Transport, form: Form) -> OutputKind {
    classify(Some(&device(name, transport, form)))
}

#[test]
fn no_device_is_unknown_and_never_warns() {
    assert_eq!(classify(None), OutputKind::Unknown);
    assert!(!should_warn(OutputKind::Unknown, true));
}

#[test]
fn only_speakers_warn_and_only_with_the_setting_on() {
    assert!(should_warn(OutputKind::Speakers, true));
    assert!(!should_warn(OutputKind::Speakers, false));
    assert!(!should_warn(OutputKind::Headphones, true));
    assert!(!should_warn(OutputKind::Unknown, true));
}

#[test]
fn built_in_speakers_are_speakers() {
    // macOS: the data source says internal speaker. Windows: form factor 1.
    // Linux: the active port is analog-output-speaker.
    for name in [
        "MacBook Pro Speakers",
        "Speakers (Realtek(R) Audio)",
        "Built-in Audio",
    ] {
        assert_eq!(
            kind(name, Transport::BuiltIn, Form::Speakers),
            OutputKind::Speakers,
            "{name}"
        );
    }
    // No form, but the name says so.
    assert_eq!(
        kind("MacBook Air Speakers", Transport::BuiltIn, Form::Unknown),
        OutputKind::Speakers
    );
}

#[test]
fn wired_headphones_are_headphones() {
    // The headphone jack: the OS says headphones even on the built-in device.
    assert_eq!(
        kind("Built-in Output", Transport::BuiltIn, Form::Headphones),
        OutputKind::Headphones
    );
    // Apple silicon lists the jack as its own device.
    assert_eq!(
        kind("External Headphones", Transport::BuiltIn, Form::Unknown),
        OutputKind::Headphones
    );
    // A USB headset by name.
    assert_eq!(
        kind("Jabra Evolve2 65 Headset", Transport::Usb, Form::Unknown),
        OutputKind::Headphones
    );
}

#[test]
fn bluetooth_is_headphones_unless_the_name_says_speaker() {
    for name in ["WH-1000XM5", "Bose QC45", "Pixel Buds Pro", "Galaxy Buds2"] {
        assert_eq!(
            kind(name, Transport::Bluetooth, Form::Unknown),
            OutputKind::Headphones,
            "{name}"
        );
    }
    for name in [
        "JBL Flip 6",
        "Bose SoundLink Flex",
        "UE WONDERBOOM 3",
        "Sonos Roam",
    ] {
        assert_eq!(
            kind(name, Transport::Bluetooth, Form::Unknown),
            OutputKind::Speakers,
            "{name}"
        );
    }
}

#[test]
fn a_bluetooth_headset_in_call_mode_is_headphones() {
    // Windows' in-box driver: "Headset (X Hands-Free AG Audio)", no
    // "Bluetooth" in the name; the bus says Bluetooth.
    assert_eq!(
        kind(
            "Headset (WH-1000XM4 Hands-Free AG Audio)",
            Transport::Bluetooth,
            Form::Headset
        ),
        OutputKind::Headphones
    );
    // Linux: the HFP profile, form from the profile.
    assert_eq!(
        kind("WH-1000XM4", Transport::Bluetooth, Form::Headset),
        OutputKind::Headphones
    );
}

#[test]
fn what_the_os_says_beats_the_name() {
    // A Bluetooth endpoint the OS calls speakers, with a headphone-ish name.
    assert_eq!(
        kind("Party Headphones", Transport::Bluetooth, Form::Speakers),
        OutputKind::Speakers
    );
    assert_eq!(
        kind("Speakers (USB Audio)", Transport::Usb, Form::Headphones),
        OutputKind::Headphones
    );
}

#[test]
fn airplay_is_speakers() {
    assert_eq!(
        kind("Living Room", Transport::AirPlay, Form::Unknown),
        OutputKind::Speakers
    );
}

#[test]
fn unclear_devices_are_unknown() {
    for (name, transport) in [
        ("Built-in Output", Transport::BuiltIn),
        ("USB Audio DAC", Transport::Usb),
        ("LG UltraFine Display Audio", Transport::Display),
        ("BlackHole 2ch", Transport::Virtual),
        ("Multi-Output Device", Transport::Virtual),
        ("", Transport::Other),
    ] {
        assert_eq!(
            kind(name, transport, Form::Unknown),
            OutputKind::Unknown,
            "{name}"
        );
    }
}

#[test]
fn name_matching_ignores_case() {
    assert!(name_suggests_headphones("AIRPODS PRO"));
    assert!(name_suggests_speaker("HomePod mini"));
    assert!(!name_suggests_headphones("Speakers"));
    assert!(!name_suggests_speaker("Headphones"));
}

#[test]
fn the_kind_serialises_for_the_log_and_the_window() {
    assert_eq!(
        serde_json::to_string(&OutputKind::Speakers).unwrap(),
        "\"speakers\""
    );
}
