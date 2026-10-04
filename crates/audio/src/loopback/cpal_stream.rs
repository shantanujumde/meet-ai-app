//! The `cpal` half of a loopback [`Backend`](super::source::Backend): the
//! two streams, built from a device the OS backend picked (TUR-38).
//!
//! `cpal` is the same API on every OS, so this builds (and its conversion is
//! tested) everywhere; which device to open, and how a callback's time maps
//! onto the host clock, stay in `crate::platform`. Linux uses it; Windows
//! (TUR-37) still has its own copy in `platform/windows_loopback.rs`.

use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    BufferSize, Data, InputCallbackInfo, SampleFormat, Stream, StreamConfig, SupportedStreamConfig,
};

use super::capture::Capture;
use super::source::LiveStream;
use crate::Error;

/// The sample formats the capture callback converts.
pub const CAPTURE_FORMATS: [SampleFormat; 3] =
    [SampleFormat::F32, SampleFormat::I16, SampleFormat::I32];

/// Samples the callback's conversion buffer starts with; grown (once) if a
/// packet is ever larger.
const CALLBACK_CONVERT_SAMPLES: usize = 16_384;

/// A running `cpal` stream; dropping it stops it.
pub struct CpalStream(Stream);

impl LiveStream for CpalStream {
    fn pause(&mut self) {
        if let Err(e) = self.0.pause() {
            tracing::debug!("pausing a loopback stream: {e}");
        }
    }
}

/// One callback's samples as `f32` in `scratch` (grown if too small), or
/// `None` for a format outside [`CAPTURE_FORMATS`].
pub fn to_f32<'a>(data: &Data, scratch: &'a mut Vec<f32>) -> Option<&'a mut [f32]> {
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
    } else {
        let samples = data.as_slice::<i32>()?;
        for (dst, src) in out.iter_mut().zip(samples) {
            *dst = (f64::from(*src) / 2_147_483_648.0) as f32;
        }
    }
    Some(out)
}

// Adapted from github.com/CapSoftware/Cap/crates/scap-cpal/src/lib.rs @ a2a6bd8b1948c48fe92936c265c8402d7fa8ddb3 (MIT)
/// A render stream of zeros on `device`, so the device keeps running (and
/// its loopback keeps delivering) while nothing else plays.
pub fn start_silence(
    device: &cpal::Device,
    supported: &SupportedStreamConfig,
    timeout: Duration,
) -> Result<Box<dyn LiveStream>, Error> {
    let mut config: StreamConfig = (*supported).into();
    config.buffer_size = BufferSize::Default;
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
            Some(timeout),
        )
        .map_err(|e| Error::NoDevice(format!("could not open the silent render stream: {e}")))?;
    stream
        .play()
        .map_err(|e| Error::NoDevice(format!("could not start the silent render stream: {e}")))?;
    Ok(Box::new(CpalStream(stream)))
}

/// What [`start_capture`] needs besides the device.
pub struct CaptureSpec<S, E> {
    /// The device's name, for errors.
    pub label: String,
    pub buffer_size: BufferSize,
    /// The capture time of a callback's first frame on the host clock.
    pub stamp: S,
    /// The stream's errors, from `cpal`'s error callback.
    pub on_error: E,
    pub timeout: Duration,
}

/// An input stream on `device` feeding `capture`, one packet per callback.
pub fn start_capture<S, E>(
    device: &cpal::Device,
    supported: &SupportedStreamConfig,
    mut capture: Capture,
    spec: CaptureSpec<S, E>,
) -> Result<Box<dyn LiveStream>, Error>
where
    S: Fn(&InputCallbackInfo) -> Option<u64> + Send + 'static,
    E: FnMut(cpal::Error) + Send + 'static,
{
    let sample_format = supported.sample_format();
    if !CAPTURE_FORMATS.contains(&sample_format) {
        return Err(Error::NoDevice(format!(
            "{} delivers {sample_format:?}, which the loopback does not convert",
            spec.label
        )));
    }
    let mut config: StreamConfig = (*supported).into();
    config.buffer_size = spec.buffer_size;
    let stamp = spec.stamp;
    let mut scratch = vec![0.0f32; CALLBACK_CONVERT_SAMPLES];
    let on_packet = move |data: &Data, info: &InputCallbackInfo| {
        if let Some(samples) = to_f32(data, &mut scratch) {
            capture.packet(samples, stamp(info), false);
        }
    };
    let stream = device
        .build_input_stream_raw(
            config,
            sample_format,
            on_packet,
            spec.on_error,
            Some(spec.timeout),
        )
        .map_err(|e| Error::NoDevice(format!("could not open the loopback stream: {e}")))?;
    stream
        .play()
        .map_err(|e| Error::NoDevice(format!("could not start the loopback stream: {e}")))?;
    Ok(Box::new(CpalStream(stream)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_of<T: cpal::SizedSample>(samples: &mut [T]) -> Data {
        // SAFETY: `samples` outlives the returned `Data` in every test, and
        // its length and format are the slice's own.
        unsafe { Data::from_parts(samples.as_mut_ptr().cast(), samples.len(), T::FORMAT) }
    }

    #[test]
    fn float_samples_pass_through() {
        let mut samples = [0.25f32, -0.5, 1.0];
        let mut scratch = Vec::new();
        let out = to_f32(&data_of(&mut samples), &mut scratch).expect("f32");
        assert_eq!(out, &[0.25, -0.5, 1.0]);
    }

    #[test]
    fn integer_samples_are_scaled_to_one() {
        let mut scratch = vec![9.0; 1];
        let mut i16s = [i16::MIN, 0, 16_384];
        let out = to_f32(&data_of(&mut i16s), &mut scratch).expect("i16");
        assert_eq!(out, &[-1.0, 0.0, 0.5]);
        let mut i32s = [i32::MIN, 1 << 30];
        let out = to_f32(&data_of(&mut i32s), &mut scratch).expect("i32");
        assert_eq!(out, &[-1.0, 0.5]);
    }

    #[test]
    fn an_unconverted_format_is_none() {
        let mut u8s = [0x80u8, 0xff];
        let mut scratch = Vec::new();
        assert!(to_f32(&data_of(&mut u8s), &mut scratch).is_none());
    }
}
