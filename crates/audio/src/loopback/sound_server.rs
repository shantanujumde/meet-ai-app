//! Which `cpal` device records "what the speakers play", per Linux sound
//! server (TUR-38). Pure, so the choice is tested on every OS; the Linux
//! backend (`platform/linux/loopback.rs`) only lists the devices.
//!
//! `cpal::default_host()` on Linux tries PipeWire, then PulseAudio, then
//! ALSA (cpal 0.18.2, `platform/mod.rs`), and each needs a different device:
//!
//! * PipeWire: cpal's own `sink_default` device. An input stream on it sets
//!   `STREAM_CAPTURE_SINK` and no `TARGET_OBJECT` (`pipewire/device.rs`,
//!   `pw_properties`), so PipeWire links it to whatever sink is the default,
//!   and moves it when the default changes.
//! * PulseAudio: the default sink's monitor source, `<sink>.monitor`, the
//!   name PulseAudio gives every sink's monitor (OBS `pulse-input.c` does
//!   the same). cpal lists monitors with the other sources.
//! * ALSA: no monitor exists at all, so no system audio (Meetily's broken
//!   Linux capture, PR #688, was this).

use cpal::{BufferSize, SupportedBufferSize};

/// The `cpal` host a device list came from, by `HostId::name()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundServer {
    PipeWire,
    PulseAudio,
    Alsa,
    Other,
}

impl SoundServer {
    /// From `cpal::HostId::name()`.
    pub fn from_host_name(name: &str) -> Self {
        match name {
            "PipeWire" => Self::PipeWire,
            "PulseAudio" => Self::PulseAudio,
            "ALSA" => Self::Alsa,
            _ => Self::Other,
        }
    }
}

/// The id cpal's PipeWire host gives its "default sink, as an input" device.
pub const PIPEWIRE_DEFAULT_SINK: &str = "sink_default";

/// What PulseAudio appends to a sink's name for its monitor source.
pub const MONITOR_SUFFIX: &str = ".monitor";

// Adapted from github.com/fastrepl/anarlog/crates/audio-actual/src/speaker/linux.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// The id (`cpal::DeviceId::id()`) of the input device to record system
/// audio from, given the host, the default output device's id, and the ids
/// of every input device the host lists. `Err` says why there is none.
pub fn system_capture_device(
    server: SoundServer,
    default_output: Option<&str>,
    inputs: &[String],
) -> Result<String, String> {
    let listed = |id: &str| inputs.iter().any(|input| input == id);
    match server {
        SoundServer::PipeWire if listed(PIPEWIRE_DEFAULT_SINK) => {
            Ok(PIPEWIRE_DEFAULT_SINK.to_string())
        }
        SoundServer::PipeWire => Err("PipeWire lists no default sink to record from".into()),
        SoundServer::PulseAudio => {
            let sink = default_output
                .ok_or_else(|| "PulseAudio has no default sink to record from".to_string())?;
            let monitor = format!("{sink}{MONITOR_SUFFIX}");
            if listed(&monitor) {
                Ok(monitor)
            } else {
                Err(format!("PulseAudio lists no monitor source {monitor:?}"))
            }
        }
        SoundServer::Alsa => Err(
            "ALSA cannot record what the speakers play; system audio needs PipeWire or \
             PulseAudio running"
                .into(),
        ),
        SoundServer::Other => Err("this audio host has no system-audio capture".into()),
    }
}

/// The start of the ALSA `null` device's description (alsa-lib's hint).
const ALSA_NULL_DESCRIPTION: &str = "Discard all samples";

/// The record fragment asked of PulseAudio, in ms: about one PipeWire
/// quantum (1024 frames at 48 kHz), so pipewire-pulse need not regroup.
pub const PULSE_FRAGMENT_MS: u64 = 20;

/// The buffer size to ask an input stream for. PulseAudio delivers a record
/// stream in fragments of whatever `fragsize` it is given, and with none
/// (`cpal`'s `BufferSize::Default`) the server picks, which can be seconds:
/// every callback then carries that much audio at once, and the track runs
/// that far ahead of its timestamps (measured in a container with
/// pipewire-pulse: 2.2 s on the microphone at 5 s). A track's position pairs
/// a packet's first-frame time with the frames through its end, so the
/// fragment is also a constant offset: PulseAudio gets
/// [`PULSE_FRAGMENT_MS`]. Every other host keeps its own default (PipeWire's
/// graph quantum, Core Audio's and WASAPI's periods).
pub fn input_buffer_size(
    server: SoundServer,
    supported: &SupportedBufferSize,
    sample_rate: u32,
) -> BufferSize {
    match server {
        SoundServer::PulseAudio => match supported {
            SupportedBufferSize::Range { min, max } if min <= max && sample_rate > 0 => {
                let frames = u64::from(sample_rate) * PULSE_FRAGMENT_MS / 1000;
                let frames = u32::try_from(frames).unwrap_or(u32::MAX);
                BufferSize::Fixed(frames.clamp(*min, *max))
            }
            _ => BufferSize::Default,
        },
        SoundServer::PipeWire | SoundServer::Alsa | SoundServer::Other => BufferSize::Default,
    }
}

/// ALSA's `null` device: it opens, "records" silence with no clock behind
/// it, and the callback spins the CPU (anarlog #7485). Never a microphone.
/// By id first, and by its description for a list that only has names.
pub fn is_alsa_null(server: SoundServer, id: &str, description: &str) -> bool {
    server == SoundServer::Alsa
        && (id == "null" || description.trim_start().starts_with(ALSA_NULL_DESCRIPTION))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn host_names_are_cpals() {
        assert_eq!(
            SoundServer::from_host_name("PipeWire"),
            SoundServer::PipeWire
        );
        assert_eq!(
            SoundServer::from_host_name("PulseAudio"),
            SoundServer::PulseAudio
        );
        assert_eq!(SoundServer::from_host_name("ALSA"), SoundServer::Alsa);
        assert_eq!(SoundServer::from_host_name("JACK"), SoundServer::Other);
    }

    #[test]
    fn pipewire_records_its_default_sink_device() {
        let inputs = ids(&["input_default", "sink_default", "alsa_input.pci.analog"]);
        assert_eq!(
            system_capture_device(SoundServer::PipeWire, Some("output_default"), &inputs),
            Ok("sink_default".to_string())
        );
        assert!(
            system_capture_device(SoundServer::PipeWire, None, &ids(&["input_default"])).is_err()
        );
    }

    #[test]
    fn pulseaudio_records_the_default_sinks_monitor() {
        let sink = "alsa_output.pci-0000_00_1f.3.analog-stereo";
        let inputs = ids(&[
            "alsa_input.pci-0000_00_1f.3.analog-stereo",
            "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
            "bluez_output.AA_BB.1.monitor",
        ]);
        assert_eq!(
            system_capture_device(SoundServer::PulseAudio, Some(sink), &inputs),
            Ok(format!("{sink}.monitor"))
        );
        // Not another sink's monitor, and not without a default sink.
        assert!(system_capture_device(SoundServer::PulseAudio, Some("hdmi"), &inputs).is_err());
        assert!(system_capture_device(SoundServer::PulseAudio, None, &inputs).is_err());
    }

    #[test]
    fn alsa_and_other_hosts_have_no_system_audio() {
        let inputs = ids(&["default", "sink_default", "x.monitor"]);
        assert!(system_capture_device(SoundServer::Alsa, Some("x"), &inputs).is_err());
        assert!(system_capture_device(SoundServer::Other, Some("x"), &inputs).is_err());
    }

    #[test]
    fn only_pulseaudio_gets_a_fixed_buffer() {
        let range = SupportedBufferSize::Range {
            min: 1,
            max: 1 << 20,
        };
        assert_eq!(
            input_buffer_size(SoundServer::PulseAudio, &range, 48_000),
            BufferSize::Fixed(960)
        );
        assert_eq!(
            input_buffer_size(
                SoundServer::PulseAudio,
                &SupportedBufferSize::Unknown,
                48_000
            ),
            BufferSize::Default
        );
        for server in [SoundServer::PipeWire, SoundServer::Alsa, SoundServer::Other] {
            assert_eq!(
                input_buffer_size(server, &range, 48_000),
                BufferSize::Default
            );
        }
    }

    #[test]
    fn the_alsa_null_device_is_recognised_by_id_or_description() {
        let null_hint = "Discard all samples (playback) or generate zero samples (capture)";
        assert!(is_alsa_null(SoundServer::Alsa, "null", ""));
        assert!(is_alsa_null(SoundServer::Alsa, "something", null_hint));
        assert!(!is_alsa_null(
            SoundServer::Alsa,
            "default",
            "Default ALSA Output (currently PipeWire Media Server)"
        ));
        // Only ALSA has it: a PipeWire node called "null" is a real one.
        assert!(!is_alsa_null(SoundServer::PipeWire, "null", null_hint));
    }
}
