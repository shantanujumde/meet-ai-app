//! System audio on Linux: an input stream on the default sink, through
//! `cpal`'s PipeWire host, or its PulseAudio host's monitor source (TUR-38).
//!
//! `cpal` 0.18.2 tries PipeWire, then PulseAudio, then ALSA. On PipeWire an
//! input stream on its `sink_default` device sets `STREAM_CAPTURE_SINK` and
//! no `TARGET_OBJECT` (`pipewire/device.rs`, `pw_properties`), so it records
//! whatever the default sink plays and follows the default when it changes.
//! On PulseAudio it is the default sink's `.monitor` source. ALSA has no
//! monitor, so there is no system audio and the recording is microphone-only
//! (contract §9). [`crate::loopback::sound_server`] makes that choice.
//!
//! This file is only the [`Backend`]: which devices, which formats. Gap
//! filling, the clock maths, silent buffers and the WAV are the shared
//! [`crate::loopback`] code, as on Windows.
//!
//! Nothing here assumes an idle sink keeps calling back (TUR-43 has not
//! measured it): a render stream of zeros keeps the sink running, and any
//! stretch with no callbacks becomes silence by the capture times.

use std::sync::{Mutex, PoisonError};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{HostId, SupportedStreamConfig};

use super::clock::input_callback_ns;
use super::devices::note_stream_lost;
use crate::loopback::capture::Capture;
use crate::loopback::clock::GapRule;
use crate::loopback::cpal_stream::{self, CaptureSpec};
use crate::loopback::sound_server::{SoundServer, input_buffer_size, system_capture_device};
use crate::loopback::source::{Backend, Format, LiveStream, LoopbackSource};
use crate::{AUDIO_PERMISSION_TIMEOUT, AudioSource, Error};

/// PipeWire's capture times come from the graph clock
/// (`pw_stream_get_time_n`), exact to the frame, and a cycle the stream
/// misses shows as a packet starting exactly one quantum late, with no data
/// for that quantum (measured in a container: one 21.3 ms jump a minute,
/// more under load, about 1 ms per second all told). So more than half a
/// packet late is a gap there.
const PIPEWIRE_GAPS: GapRule = GapRule {
    floor_ns: 2_000_000,
    packet_percent: 50,
};

/// PulseAudio's capture times are cpal's latency estimate, which wanders by
/// tens of ms between polls (measured: plus or minus 25 ms), so only a long
/// silence counts. Measure it with TUR-43.
const PULSEAUDIO_GAPS: GapRule = GapRule {
    floor_ns: 100_000_000,
    packet_percent: 100,
};

/// The gap rule for a sound server's capture times.
fn gap_rule_for(server: Option<SoundServer>) -> GapRule {
    match server {
        Some(SoundServer::PipeWire) => PIPEWIRE_GAPS,
        Some(SoundServer::PulseAudio) => PULSEAUDIO_GAPS,
        Some(SoundServer::Alsa | SoundServer::Other) | None => GapRule::WASAPI,
    }
}

/// The loopback source on the default sink.
pub(crate) fn system_source() -> Option<Box<dyn AudioSource>> {
    Some(Box::new(LoopbackSource::new(LinuxLoopback::default())))
}

/// The capture device and the default output (for the keepalive), read at
/// [`Backend::format`].
#[derive(Default)]
struct LinuxLoopback {
    capture: Option<(cpal::Device, SupportedStreamConfig)>,
    output: Option<cpal::Device>,
    server: Option<SoundServer>,
}

/// `cpal::default_host()`, keeping a PipeWire host alive for the rest of the
/// process. Each PipeWire host holds libpipewire initialised, and the last
/// one dropped calls `pw_deinit()`; anarlog found a later `pw_init()` after
/// that unsafe to rely on, so the library stays loaded once used.
fn host() -> cpal::Host {
    static PIPEWIRE: Mutex<Option<cpal::Host>> = Mutex::new(None);
    let host = cpal::default_host();
    if host.id() == HostId::PipeWire {
        let mut kept = PIPEWIRE.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.is_none() {
            *kept = cpal::host_from_id(HostId::PipeWire).ok();
        }
    }
    host
}

fn device_id(device: &cpal::Device) -> String {
    device
        .id()
        .map(|id| id.id().to_string())
        .unwrap_or_default()
}

impl LinuxLoopback {
    fn opened(&self) -> Result<&(cpal::Device, SupportedStreamConfig), Error> {
        self.capture
            .as_ref()
            .ok_or_else(|| Error::NoDevice("the sink was not read first".into()))
    }
}

impl Backend for LinuxLoopback {
    fn format(&mut self) -> Result<Format, Error> {
        let host = host();
        let server = SoundServer::from_host_name(host.id().name());
        let output = host.default_output_device();
        let output_id = output.as_ref().map(device_id);
        let inputs: Vec<cpal::Device> = match server {
            SoundServer::PipeWire | SoundServer::PulseAudio => host
                .input_devices()
                .map_err(|e| Error::NoDevice(format!("could not list the input devices: {e}")))?
                .collect(),
            SoundServer::Alsa | SoundServer::Other => Vec::new(),
        };
        let ids: Vec<String> = inputs.iter().map(device_id).collect();
        let wanted =
            system_capture_device(server, output_id.as_deref(), &ids).map_err(Error::NoDevice)?;
        let device = inputs
            .into_iter()
            .zip(&ids)
            .find_map(|(device, id)| (*id == wanted).then_some(device))
            .ok_or_else(|| Error::NoDevice(format!("{wanted} is no longer listed")))?;
        let supported = device
            .default_input_config()
            .map_err(|e| Error::NoDevice(format!("no usable config on {wanted}: {e}")))?;
        let label = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| wanted.clone());
        let format = Format {
            rate: supported.sample_rate(),
            channels: usize::from(supported.channels()),
            label: format!("{label} ({} {wanted})", host.id().name()),
        };
        tracing::info!(
            "system loopback: {:?} samples, buffer sizes {:?}",
            supported.sample_format(),
            supported.buffer_size()
        );
        self.capture = Some((device, supported));
        self.output = output;
        self.server = Some(server);
        Ok(format)
    }

    fn start_keepalive(&mut self) -> Result<Box<dyn LiveStream>, Error> {
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| Error::NoDevice("no default output to keep running".into()))?;
        let supported = output
            .default_output_config()
            .map_err(|e| Error::NoDevice(format!("no usable output config: {e}")))?;
        cpal_stream::start_silence(output, &supported, AUDIO_PERMISSION_TIMEOUT)
    }

    fn start_capture(
        &mut self,
        format: &Format,
        capture: Capture,
    ) -> Result<Box<dyn LiveStream>, Error> {
        let (device, supported) = self.opened()?;
        let spec = CaptureSpec {
            label: format.label.clone(),
            // PipeWire keeps the graph's own quantum (a fixed size would set
            // the node latency of every stream grouped with this one);
            // PulseAudio gets about 80 ms instead of the server's fragment.
            buffer_size: input_buffer_size(
                self.server.unwrap_or(SoundServer::Other),
                supported.buffer_size(),
                supported.sample_rate(),
            ),
            stamp: input_callback_ns,
            on_error: on_stream_error,
            timeout: AUDIO_PERMISSION_TIMEOUT,
        };
        cpal_stream::start_capture(device, supported, capture, spec)
    }

    fn gap_rule(&self) -> GapRule {
        gap_rule_for(self.server)
    }
}

/// The capture stream's errors. One whose device went away (the sink was
/// removed) or that `cpal` gave up on asks the session for a new segment.
/// `DeviceChanged` is not a loss: PipeWire moved the stream to the new
/// default, and the device watch opens that segment.
fn on_stream_error(error: cpal::Error) {
    if cpal_stream::stream_is_lost(error.kind()) {
        tracing::warn!("system loopback stream lost: {error}; opening a new segment");
        note_stream_lost();
    } else {
        tracing::info!("system loopback stream: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_sound_server_gets_its_gap_rule() {
        assert_eq!(gap_rule_for(Some(SoundServer::PipeWire)), PIPEWIRE_GAPS);
        assert_eq!(gap_rule_for(Some(SoundServer::PulseAudio)), PULSEAUDIO_GAPS);
        assert_eq!(gap_rule_for(None), GapRule::WASAPI);
    }

    #[test]
    fn the_system_source_opens_or_fails_without_panicking() {
        // CI has no sound server: format() fails, which the session turns
        // into a microphone-only recording.
        let mut backend = LinuxLoopback::default();
        match backend.format() {
            Ok(format) => eprintln!("system loopback would record {format:?}"),
            Err(error) => eprintln!("no system loopback here: {error}"),
        }
    }
}
