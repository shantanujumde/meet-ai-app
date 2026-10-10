//! System audio on Windows: WASAPI loopback through `cpal` (TUR-37).
//!
//! `cpal` 0.18.2 opens an *input* stream on an *output* device as a loopback
//! capture: `build_input_stream_raw_inner` adds `AUDCLNT_STREAMFLAGS_LOOPBACK`
//! when the device's data flow is `eRender` (`wasapi/device.rs`). This is
//! endpoint loopback, everything the default output plays, not per-process
//! loopback, which returns silence for the Teams desktop app. When other apps
//! play to one non-default endpoint only (a call on a headset that is not the
//! default), that endpoint is opened instead (TUR-95, [`followed_device`]).
//!
//! This file is only the [`Backend`]: which device and which format. The two
//! streams are the shared [`cpal_stream`] ones, as on Linux (TUR-177); gap
//! filling, the clock, silent buffers and the WAV are the shared
//! [`crate::loopback`] code, tested on every OS with a fake backend.
//!
//! The capture time each packet carries is `GetBuffer`'s QPC position
//! (`wasapi/stream.rs`, `input_timestamp`), the same clock as
//! `windows_devices::host_now_ns` and the microphone's callbacks.

use cpal::SupportedStreamConfig;
use cpal::traits::{DeviceTrait, HostTrait};

use super::windows_devices::{
    current_default_output_endpoint, followed_output_endpoint, host, input_callback_ns,
};
use super::windows_render_choice::endpoint_to_open;
use crate::loopback::buffer::safe_buffer_size;
use crate::loopback::capture::Capture;
use crate::loopback::cpal_stream::{self, CaptureSpec};
use crate::loopback::source::{Backend, Format, LiveStream, LoopbackSource};
use crate::{AUDIO_PERMISSION_TIMEOUT, AudioSource, Error};

/// The loopback source on the followed output device.
pub(crate) fn system_source() -> Option<Box<dyn AudioSource>> {
    Some(Box::new(LoopbackSource::new(WasapiLoopback::default())))
}

/// The output device to record (TUR-95): the endpoint the device watch
/// follows when it is not the default (a call playing to a headset that is
/// not the default), opened by id; otherwise `cpal`'s default device, which
/// Windows reroutes by itself when the default changes. An endpoint that has
/// gone away since the watch last read falls back to the default.
fn followed_device() -> Result<cpal::Device, Error> {
    let host = host();
    let followed = followed_output_endpoint();
    let default = current_default_output_endpoint();
    if let Some(id) = endpoint_to_open(followed.as_deref(), default.as_deref()) {
        let by_id = host.output_devices().ok().and_then(|mut devices| {
            devices.find(|device| device.id().is_ok_and(|device_id| device_id.id() == id))
        });
        match by_id {
            Some(device) => {
                tracing::info!("system loopback: following {id}, which other apps play to");
                return Ok(device);
            }
            None => tracing::info!("system loopback: {id} is gone; recording the default"),
        }
    }
    host.default_output_device().ok_or_else(|| {
        Error::NoDevice("no default output device to record system audio from".into())
    })
}

/// The followed output device and the format read from it at [`Backend::format`].
#[derive(Default)]
struct WasapiLoopback {
    device: Option<(cpal::Device, SupportedStreamConfig)>,
}

impl WasapiLoopback {
    fn opened(&self) -> Result<&(cpal::Device, SupportedStreamConfig), Error> {
        self.device
            .as_ref()
            .ok_or_else(|| Error::NoDevice("the output device was not read first".into()))
    }
}

impl Backend for WasapiLoopback {
    fn format(&mut self) -> Result<Format, Error> {
        let device = followed_device()?;
        let supported = device
            .default_output_config()
            .map_err(|e| Error::NoDevice(format!("no usable output config: {e}")))?;
        let label = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "the default output device".to_string());
        let format = Format {
            rate: supported.sample_rate(),
            channels: usize::from(supported.channels()),
            label,
        };
        tracing::info!(
            "system loopback: {:?} samples, buffer sizes {:?}",
            supported.sample_format(),
            supported.buffer_size()
        );
        self.device = Some((device, supported));
        Ok(format)
    }

    fn start_keepalive(&mut self) -> Result<Box<dyn LiveStream>, Error> {
        let (device, supported) = self.opened()?;
        cpal_stream::start_silence(device, supported, AUDIO_PERMISSION_TIMEOUT)
    }

    fn start_capture(
        &mut self,
        format: &Format,
        capture: Capture,
    ) -> Result<Box<dyn LiveStream>, Error> {
        let (device, supported) = self.opened()?;
        let buffer_size = safe_buffer_size(supported.buffer_size(), supported.sample_rate());
        tracing::info!("system loopback: asking for {buffer_size:?} buffers");
        let spec = CaptureSpec {
            label: format.label.clone(),
            buffer_size,
            stamp: input_callback_ns,
            // A default-device change surfaces here as StreamInvalidated while
            // Windows reroutes the stream; the device watch opens the new segment.
            on_error: |e: cpal::Error| tracing::info!("system loopback stream: {e}"),
            timeout: AUDIO_PERMISSION_TIMEOUT,
        };
        cpal_stream::start_capture(device, supported, capture, spec)
    }
}
