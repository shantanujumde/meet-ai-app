//! Each OS's raw answers, mapped on every OS (TUR-65).

use super::*;

#[test]
fn mac_data_source_decides() {
    assert_eq!(
        mac_form(Some(DATA_SOURCE_INTERNAL_SPEAKER), &[]),
        Form::Speakers
    );
    assert_eq!(
        mac_form(Some(DATA_SOURCE_EXTERNAL_SPEAKER), &[]),
        Form::Speakers
    );
    assert_eq!(
        mac_form(Some(DATA_SOURCE_HEADPHONES), &[]),
        Form::Headphones
    );
    // The data source wins over a stream's terminal type.
    assert_eq!(
        mac_form(Some(DATA_SOURCE_INTERNAL_SPEAKER), &[TERMINAL_HEADPHONES]),
        Form::Speakers
    );
}

#[test]
fn mac_four_char_codes_are_the_header_values() {
    assert_eq!(DATA_SOURCE_INTERNAL_SPEAKER, 0x6973_706b);
    assert_eq!(DATA_SOURCE_HEADPHONES, 0x6864_706e);
    assert_eq!(TERMINAL_HEADPHONES, 0x6864_7068);
}

#[test]
fn mac_terminal_types_only_ever_say_headphones() {
    assert_eq!(mac_form(None, &[TERMINAL_HEADPHONES]), Form::Headphones);
    assert_eq!(mac_form(None, &[0x0302]), Form::Headphones);
    assert_eq!(mac_form(None, &[0x0303]), Form::Headphones);
    assert_eq!(mac_form(None, &[0x0402]), Form::Headset);
    // 'spkr' and USB speaker (0x0301) are not trusted: unknown.
    assert_eq!(
        mac_form(None, &[u32::from_be_bytes(*b"spkr")]),
        Form::Unknown
    );
    assert_eq!(mac_form(None, &[0x0301]), Form::Unknown);
    // Line out as the data source, no streams: unknown.
    assert_eq!(
        mac_form(Some(u32::from_be_bytes(*b"line")), &[]),
        Form::Unknown
    );
    assert_eq!(mac_form(None, &[]), Form::Unknown);
}

#[test]
fn windows_form_factors() {
    assert_eq!(windows_form(Some(1)), Form::Speakers);
    assert_eq!(windows_form(Some(3)), Form::Headphones);
    assert_eq!(windows_form(Some(5)), Form::Headset);
    assert_eq!(windows_form(Some(7)), Form::Headset);
    // RemoteNetworkDevice, LineLevel, SPDIF, DigitalAudioDisplayDevice,
    // UnknownFormFactor, and no value.
    for factor in [Some(0), Some(2), Some(8), Some(9), Some(10), None] {
        assert_eq!(windows_form(factor), Form::Unknown, "{factor:?}");
    }
}

#[test]
fn windows_adapter_paths_name_their_bus() {
    for (path, expected) in [
        (
            r"{2}.\\?\bthhfenum#bthhfpaudio#8&1f4a0b2c&0&97#{6994ad04-93ef-11d0-a3cc-00a0c9223196}",
            Transport::Bluetooth,
        ),
        (
            r"{2}.\\?\BTHENUM#{0000110b-0000-1000-8000-00805f9b34fb}_LOCALMFG&0002#7&1&0&B8D50B#{6994ad04}",
            Transport::Bluetooth,
        ),
        (
            r"{2}.\\?\usb#vid_0b0e&pid_0300&mi_00#7&2a1c&0&0000#{6994ad04}",
            Transport::Usb,
        ),
        (
            r"{2}.\\?\hdaudio#func_01&ven_10ec&dev_0236#4&1b1&0&0001#{6994ad04}",
            Transport::BuiltIn,
        ),
        (
            r"{2}.\\?\swd#mmdevapi#{0.0.0.00000000}#{6994ad04}",
            Transport::Virtual,
        ),
    ] {
        let enumerator = adapter_enumerator(path);
        assert_eq!(windows_transport(enumerator, ""), expected, "{path}");
    }
    assert_eq!(adapter_enumerator("no device path here"), None);
}

#[test]
fn windows_falls_back_to_the_name() {
    assert_eq!(
        windows_transport(None, "Headset (WH-1000XM4 Hands-Free AG Audio)"),
        Transport::Bluetooth
    );
    assert_eq!(
        windows_transport(Some("unknownbus"), "Speakers (USB Audio Device)"),
        Transport::Usb
    );
    assert_eq!(
        windows_transport(None, "LG HDR 4K (NVIDIA High Definition Audio) HDMI"),
        Transport::Display
    );
    assert_eq!(windows_transport(None, "Speakers"), Transport::Other);
}

fn sink(name: &str, port: Option<&str>) -> PulseSink {
    PulseSink {
        name: name.to_owned(),
        active_port: port.map(str::to_owned),
        ..PulseSink::default()
    }
}

#[test]
fn pulse_ports_decide() {
    let laptop = "alsa_output.pci-0000_00_1f.3.analog-stereo";
    assert_eq!(
        pulse_form(&sink(laptop, Some("analog-output-speaker"))),
        Form::Speakers
    );
    assert_eq!(
        pulse_form(&sink(laptop, Some("analog-output-headphones"))),
        Form::Headphones
    );
    assert_eq!(
        pulse_form(&sink(laptop, Some("[Out] Headphones"))),
        Form::Headphones
    );
    assert_eq!(
        pulse_form(&sink(laptop, Some("analog-output-lineout"))),
        Form::Unknown
    );
    let bluez = "bluez_output.AC_80_0A_00_00_00.1";
    assert_eq!(
        pulse_form(&sink(bluez, Some("headphone-output"))),
        Form::Headphones
    );
    assert_eq!(
        pulse_form(&sink(bluez, Some("headset-output"))),
        Form::Headset
    );
    assert_eq!(
        pulse_form(&sink(bluez, Some("speaker-output"))),
        Form::Speakers
    );
}

#[test]
fn pulse_headset_profile_and_form_factor() {
    for profile in [
        "headset_head_unit",
        "handsfree_head_unit",
        "headset-head-unit",
    ] {
        let mut s = sink("bluez_output.X.1", Some("bluetooth-output"));
        s.bluetooth_profile = Some(profile.to_owned());
        assert_eq!(pulse_form(&s), Form::Headset, "{profile}");
    }
    let mut a2dp = sink("bluez_output.X.1", None);
    a2dp.bluetooth_profile = Some("a2dp_sink".to_owned());
    assert_eq!(pulse_form(&a2dp), Form::Unknown);
    a2dp.form_factor = Some("headphone".to_owned());
    assert_eq!(pulse_form(&a2dp), Form::Headphones);
    a2dp.form_factor = Some("speaker".to_owned());
    assert_eq!(pulse_form(&a2dp), Form::Speakers);
    // "internal" says where the card is, not what plays.
    a2dp.form_factor = Some("internal".to_owned());
    assert_eq!(pulse_form(&a2dp), Form::Unknown);
}

#[test]
fn pulse_transport_from_bus_then_name() {
    let mut s = sink("alsa_output.usb-Generic_USB_Audio-00.analog-stereo", None);
    assert_eq!(pulse_transport(&s), Transport::Usb);
    s.bus = Some("bluetooth".to_owned());
    assert_eq!(pulse_transport(&s), Transport::Bluetooth);
    assert_eq!(
        pulse_transport(&sink("bluez_output.X.1", None)),
        Transport::Bluetooth
    );
    assert_eq!(
        pulse_transport(&sink("alsa_output.pci-0000_01_00.1.hdmi-stereo", None)),
        Transport::Display
    );
    assert_eq!(
        pulse_transport(&sink("alsa_output.pci-0000_00_1f.3.analog-stereo", None)),
        Transport::Other
    );
}
