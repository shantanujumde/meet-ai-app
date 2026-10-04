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
//! This file is only the [`Backend`]: which device, which format, the two
//! streams. Gap filling, the clock, silent buffers and the WAV are the shared
//! [`crate::loopback`] code, tested on every OS with a fake backend.
//!
//! The capture time each packet carries is `GetBuffer`'s QPC position
//! (`wasapi/stream.rs`, `input_timestamp`), the same clock as
//! `windows_devices::host_now_ns` and the microphone's callbacks.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, InputCallbackInfo, SampleFormat, Stream, StreamConfig, SupportedStreamConfig};

use super::windows_devices::{
    current_default_output_endpoint, followed_output_endpoint, host, input_callback_ns,
};
use super::windows_render_choice::endpoint_to_open;
use crate::loopback::buffer::safe_buffer_size;
use crate::loopback::capture::Capture;
use crate::loopback::source::{Backend, Format, LiveStream, LoopbackSource};
use crate::{AUDIO_PERMISSION_TIMEOUT, AudioSource, Error};

/// The sample formats the capture callback converts. WASAPI's shared-mode
/// mix format is 32-bit float on every Windows release since Vista; the
/// integer ones are there for a driver that says otherwise.
const CAPTURE_FORMATS: [SampleFormat; 3] =
    [SampleFormat::F32, SampleFormat::I16, SampleFormat::I32];

/// Samples the callback's conversion buffer starts with; grown (once) if a
/// packet is ever larger.
const CALLBACK_CONVERT_SAMPLES: usize = 16_384;

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

/// A running `cpal` stream; dropping it stops it.
struct CpalStream(Stream);

impl LiveStream for CpalStream {
    fn pause(&mut self) {
        if let Err(e) = self.0.pause() {
            tracing::debug!("pausing a loopback stream: {e}");
        }
    }
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

    // Adapted from github.com/CapSoftware/Cap/crates/scap-cpal/src/lib.rs @ a2a6bd8b1948c48fe92936c265c8402d7fa8ddb3 (MIT)
    fn start_keepalive(&mut self) -> Result<Box<dyn LiveStream>, Error> {
        let (device, supported) = self.opened()?;
        let mut config: StreamConfig = (*supported).into();
        config.buffer_size = cpal::BufferSize::Default;
        let sample_format = supported.sample_format();
        // Unsigned PCM's silence is the middle of its range, not 0.
        let fill = if sample_format == SampleFormat::U8 {
            0x80
        } else {
            0
        };
        let stream = device
            .build_output_stream_raw(
                config,
                sample_format,
                move |data: &mut Data, _| data.bytes_mut().fill(fill),
                |e| tracing::debug!("loopback silence keepalive: {e}"),
                Some(AUDIO_PERMISSION_TIMEOUT),
            )
            .map_err(|e| {
                Error::NoDevice(format!("could not open the silent render stream: {e}"))
            })?;
        stream.play().map_err(|e| {
            Error::NoDevice(format!("could not start the silent render stream: {e}"))
        })?;
        Ok(Box::new(CpalStream(stream)))
    }

    fn start_capture(
        &mut self,
        format: &Format,
        mut capture: Capture,
    ) -> Result<Box<dyn LiveStream>, Error> {
        let (device, supported) = self.opened()?;
        let sample_format = supported.sample_format();
        if !CAPTURE_FORMATS.contains(&sample_format) {
            return Err(Error::NoDevice(format!(
                "{} mixes in {sample_format:?}, which the loopback does not convert",
                format.label
            )));
        }
        let mut config: StreamConfig = (*supported).into();
        config.buffer_size = safe_buffer_size(supported.buffer_size(), supported.sample_rate());
        tracing::info!(
            "system loopback: asking for {:?} buffers",
            config.buffer_size
        );

        let mut scratch = vec![0.0f32; CALLBACK_CONVERT_SAMPLES];
        let on_packet = move |data: &Data, info: &InputCallbackInfo| {
            let n = data.len();
            if scratch.len() < n {
                scratch.resize(n, 0.0);
            }
            let out = &mut scratch[..n];
            if let Some(samples) = data.as_slice::<f32>() {
                out.copy_from_slice(samples);
            } else if let Some(samples) = data.as_slice::<i16>() {
                for (dst, src) in out.iter_mut().zip(samples) {
                    *dst = f32::from(*src) / 32_768.0;
                }
            } else if let Some(samples) = data.as_slice::<i32>() {
                for (dst, src) in out.iter_mut().zip(samples) {
                    *dst = (f64::from(*src) / 2_147_483_648.0) as f32;
                }
            } else {
                return;
            }
            // cpal 0.18.2 does not pass AUDCLNT_BUFFERFLAGS_SILENT on.
            capture.packet(out, input_callback_ns(info), false);
        };
        let on_error = |e: cpal::Error| {
            // A default-device change surfaces here as StreamInvalidated while
            // Windows reroutes the stream; the device watch opens the new segment.
            tracing::info!("system loopback stream: {e}");
        };
        let stream = device
            .build_input_stream_raw(
                config,
                sample_format,
                on_packet,
                on_error,
                Some(AUDIO_PERMISSION_TIMEOUT),
            )
            .map_err(|e| Error::NoDevice(format!("could not open the loopback stream: {e}")))?;
        stream
            .play()
            .map_err(|e| Error::NoDevice(format!("could not start the loopback stream: {e}")))?;
        Ok(Box::new(CpalStream(stream)))
    }
}
