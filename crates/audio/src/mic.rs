//! Microphone capture via `cpal` (SPEC §2.3).
//!
//! `cpal` is already cross-platform (SETUP.md: pinned at 0.18.2 for exactly
//! this reason), so unlike the process tap — macOS-only, lives in
//! [`crate::macos`] per SPEC §4's ⛔ — this module needs no platform gate of
//! its own. The one exception is the capture time each callback stamps its
//! packet with, which is OS code and comes from `crate::platform`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};

use crate::capture_clock::{CaptureClock, Feed, first_frame_ns, time_marks};
use crate::loopback::sound_server::{SoundServer, input_buffer_size, is_alsa_null};
use crate::mic_choice::{self, MicChoice};
use crate::pipeline::Pipeline;
use crate::platform::host_now_ns;
// The OS's capture time, on the host clock: WASAPI's on Windows (TUR-37),
// Core Audio's on macOS (TUR-151).
use crate::platform::input_callback_ns;
use crate::rate_meter::{CallbackMeter, FixedRates, Rates};
use crate::tee::Tee;
use crate::track::{IDLE_POLL, TrackWriter};
use crate::worker::Worker;
use crate::{AudioSource, Channel, Error};

/// Ring buffer capacity, in raw (device-rate, interleaved) samples. 4 s at
/// 48 kHz stereo is generously oversized for a worker thread serviced every
/// couple of milliseconds; it exists so a scheduling hiccup drops nothing
/// rather than to hold steady-state backlog.
const RING_CAPACITY_SAMPLES: usize = 48_000 * 2 * 4;

/// Everything [`MicSource::start`] needs to hand back once the stream is
/// actually live.
///
/// Dropped whole, as a build that finished after [`MicSource::start`] gave
/// up on it is (TUR-162), it tears itself down: the fields drop in order, so
/// the stream stops (`cpal`'s own drop) and then the [`Worker`] is joined.
struct Built {
    stream: Stream,
    rates: Arc<FixedRates>,
    worker: Worker,
    track: TrackWriter,
}

/// The microphone capture channel: a `cpal` input stream feeding a resampler
/// and a [`crate::wav_writer::WavWriter`] through a lock-free ring buffer, per SPEC §2.3's
/// requirement that the real-time audio callback never blocks, allocates, or
/// touches the filesystem.
pub struct MicSource {
    stream: Option<Stream>,
    worker: Option<Worker>,
    track: Option<TrackWriter>,
    /// The live-transcription copy, if one was asked for ([`AudioSource::tee`]).
    tee: Option<Tee>,
    /// The reported and measured device rates (TUR-87), while running.
    rates: Option<Arc<FixedRates>>,
}

impl Default for MicSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MicSource {
    pub fn new() -> Self {
        Self {
            stream: None,
            worker: None,
            track: None,
            tee: None,
            rates: None,
        }
    }

    /// Raw device samples → [`Pipeline`], following the measured rate
    /// (TUR-87): when the callbacks deliver another rate than `cpal`
    /// reported, the resampler is rebuilt at the measured one. Every append
    /// carries the capture time of the frame it ends at (TUR-151).
    fn worker_loop(
        mut consumer: HeapCons<f32>,
        mut pipeline: Pipeline,
        rates: Arc<FixedRates>,
        track: TrackWriter,
        clock: CaptureClock,
        running: Arc<AtomicBool>,
        tee: Option<Tee>,
    ) {
        let mut feed = Feed::new(track, clock, tee);
        // Sized once; `pop_slice` only refills it.
        let mut scratch = vec![0.0f32; 4096];
        loop {
            let popped = consumer.pop_slice(&mut scratch);
            if popped == 0 && !running.load(Ordering::Acquire) {
                break;
            } else if popped == 0 {
                std::thread::sleep(IDLE_POLL);
                continue;
            }
            feed.push(&mut pipeline, &*rates, &scratch[..popped]);
        }
    }

    /// Stop the stream and join the worker; the track stays, so
    /// [`AudioSource::position`] still answers and the header can be patched.
    fn halt(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
        }
        if let Some(mut worker) = self.worker.take() {
            worker.halt();
        }
    }
}

impl Drop for MicSource {
    fn drop(&mut self) {
        // TUR-162: never leave the worker polling an abandoned ring.
        self.halt();
    }
}

impl MicSource {
    /// Everything that can block on the mic's TCC consent dialog, run on its
    /// own thread so [`AudioSource::start`] can bound the wait.
    ///
    /// ⚠️ Measured directly (TUR-4): `cpal`'s coreaudio backend does **not**
    /// honor the `timeout` parameter of `build_input_stream` for the call
    /// that actually blocks on permission (`AudioUnitInitialize`, inside
    /// `audio_unit_from_device`) — reading `cpal` 0.18.2's source, that
    /// parameter is only consulted by a sample-rate-negotiation fallback path
    /// that has nothing to do with TCC. `Some(AUDIO_PERMISSION_TIMEOUT)` is
    /// still passed below because it costs nothing and helps on backends that
    /// do honor it, but it is not what makes this method time-bounded. This
    /// function running on its own thread, joined with `recv_timeout` in
    /// [`MicSource::start`], is what does that: if the dialog is never
    /// answered, this thread stays blocked forever, but the caller gets an
    /// [`Error`] back instead of hanging with it.
    fn build(dest: PathBuf, tee: Option<Tee>) -> Result<Built, Error> {
        let host = cpal::default_host();
        let device = pick_device(&host)?;
        let supported = device
            .default_input_config()
            .map_err(|e| Error::NoDevice(format!("no usable input config: {e}")))?;
        let sample_format = supported.sample_format();
        // TUR-38: PulseAudio's own record fragment can be seconds long.
        let buffer_size = input_buffer_size(
            SoundServer::from_host_name(host.id().name()),
            supported.buffer_size(),
            supported.sample_rate(),
        );
        let mut config: StreamConfig = supported.into();
        config.buffer_size = buffer_size;
        let device_rate = config.sample_rate;
        let channels = config.channels as usize;
        // Read once from `cpal`, so it can go stale (a headset mic switching to
        // HFP when another app opens a call): logged, and measured below.
        tracing::info!(
            "microphone device rate {device_rate} Hz, {channels} ch, {sample_format:?} \
             (cpal default input config)"
        );
        let rates = FixedRates::new("microphone", device_rate);

        // A segment reopen (device change) restarts capture against the same
        // `dest` a previous `MicSource` already wrote to — the WAV stays one
        // continuous per-channel archive across segments (contract:
        // "concatenated in idx order"), only the OS-level stream is rebuilt.
        // `TrackWriter::open` creates the file or appends to it, with a
        // segment-relative frame count (see its doc).
        let track = TrackWriter::open(&dest, "microphone")?;

        let rb = HeapRb::<f32>::new(RING_CAPACITY_SAMPLES);
        let (mut producer, consumer) = rb.split();
        // Each packet's capture time rides next to its samples (TUR-151).
        let (mut marks, clock) = time_marks(channels);

        // The delivered-rate meter (TUR-84's, TUR-87): integers and atomics.
        let mut meter = CallbackMeter::new(Arc::clone(&rates), channels);
        let err_fn = |err| tracing::warn!("cpal input stream error: {err}");

        // The callback itself: timestamp, then push into the lock-free ring.
        // No allocation, no lock, no I/O — SPEC §2.3's real-time constraint.
        // The time is the packet's first frame's capture, or the callback's
        // own less the packet when the OS gives none (`first_frame_ns`).
        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                    let frames = data.len() / channels.max(1);
                    let now =
                        first_frame_ns(input_callback_ns(info), host_now_ns(), frames, device_rate);
                    marks.mark(now);
                    marks.advance(producer.push_slice(data));
                    meter.observe(now, data.len());
                },
                err_fn,
                Some(crate::AUDIO_PERMISSION_TIMEOUT),
            ),
            SampleFormat::I16 => {
                let mut scratch = vec![0.0f32; 8192];
                device.build_input_stream(
                    config,
                    move |data: &[i16], info: &cpal::InputCallbackInfo| {
                        let frames = data.len() / channels.max(1);
                        let now = first_frame_ns(
                            input_callback_ns(info),
                            host_now_ns(),
                            frames,
                            device_rate,
                        );
                        meter.observe(now, data.len());
                        if scratch.len() < data.len() {
                            scratch.resize(data.len(), 0.0);
                        }
                        for (dst, src) in scratch.iter_mut().zip(data.iter()) {
                            *dst = *src as f32 / i16::MAX as f32;
                        }
                        marks.mark(now);
                        marks.advance(producer.push_slice(&scratch[..data.len()]));
                    },
                    err_fn,
                    Some(crate::AUDIO_PERMISSION_TIMEOUT),
                )
            }
            other => {
                return Err(Error::NoDevice(format!(
                    "microphone reports an unsupported sample format: {other:?}"
                )));
            }
        }
        .map_err(|e| Error::NoDevice(format!("failed to build input stream: {e}")))?;

        stream
            .play()
            .map_err(|e| Error::NoDevice(format!("failed to start input stream: {e}")))?;

        let worker = Worker::spawn("meet-rec-mic-worker", {
            let track = track.clone();
            let rates = Arc::clone(&rates);
            let pipeline = Pipeline::new("microphone", channels, device_rate);
            move |running| Self::worker_loop(consumer, pipeline, rates, track, clock, running, tee)
        })?;

        Ok(Built {
            stream,
            rates,
            worker,
            track,
        })
    }
}

impl AudioSource for MicSource {
    fn start(&mut self, dest: PathBuf) -> Result<(), Error> {
        let tee = self.tee.clone();
        // On a timeout the init thread is still blocked inside Core Audio
        // and has no way to be cancelled (see `MicSource::build`'s doc), so
        // it is leaked rather than joined: the alternative is this method
        // hanging with it. If it finishes later, its `Built` is dropped
        // there and stops itself (TUR-162).
        let built = crate::init_thread::run_bounded(
            "meet-rec-mic-init",
            crate::AUDIO_PERMISSION_TIMEOUT,
            move || Self::build(dest, tee),
        )?;

        self.stream = Some(built.stream);
        self.rates = Some(built.rates);
        self.worker = Some(built.worker);
        self.track = Some(built.track);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.stop_capture()?;
        self.patch_header()
    }

    fn stop_capture(&mut self) -> Result<(), Error> {
        self.halt();
        self.fsync_data()
    }

    fn channel(&self) -> Channel {
        Channel::Mic
    }

    fn position(&self) -> Option<(u64, u64)> {
        self.track.as_ref()?.position()
    }

    fn fsync_data(&mut self) -> Result<(), Error> {
        if let Some(track) = &self.track {
            track.fsync_data()?;
        }
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), Error> {
        if let Some(track) = &self.track {
            track.patch_header()?;
        }
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), Error> {
        // The track's lock excludes the worker's appends for the splice.
        if let Some(track) = &self.track {
            track.pad_leading_silence(frames, self.tee.as_ref())?;
        }
        Ok(())
    }

    fn tee(&mut self, tee: Tee) {
        self.tee = Some(tee);
    }

    fn rate_report(&self) -> Option<String> {
        self.rates.as_ref().map(|rates| rates.describe())
    }

    fn device_rate(&self) -> Option<u32> {
        self.rates.as_ref().map(|rates| rates.effective())
    }
}

/// The input device to open ([`choose_device`]), never ALSA's `null`
/// device: it opens, "records" silence with no clock behind it, and its
/// callback spins the CPU (TUR-38, anarlog #7485).
fn pick_device(host: &cpal::Host) -> Result<cpal::Device, Error> {
    let device = choose_device(host)?;
    let server = SoundServer::from_host_name(host.id().name());
    let id = device
        .id()
        .map(|id| id.id().to_string())
        .unwrap_or_default();
    let description = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default();
    if is_alsa_null(server, &id, &description) {
        return Err(Error::NoDevice(
            "the default input is ALSA's null device, which records only silence; \
             choose a real microphone as the default input"
                .to_string(),
        ));
    }
    Ok(device)
}

/// The input device to open: the default, unless [`mic_choice::choose`]
/// picks another (TUR-91: the Mac's own mic when the default is Bluetooth).
/// The choice and its reason are logged; any failure to list or find the
/// chosen device falls back to the default, as before.
fn choose_device(host: &cpal::Host) -> Result<cpal::Device, Error> {
    let default = || {
        host.default_input_device()
            .ok_or_else(|| Error::NoDevice("no default input device".to_string()))
    };
    let devices = match crate::platform::input_devices() {
        Ok(devices) => devices,
        Err(Error::Unsupported) => return default(),
        Err(error) => {
            tracing::info!("microphone: the default input ({error} while listing devices)");
            return default();
        }
    };
    match mic_choice::choose(&devices, mic_choice::use_builtin_with_bluetooth()) {
        MicChoice::Default { reason } => {
            let name = devices
                .iter()
                .find(|d| d.is_default)
                .map(|d| d.name.as_str());
            tracing::info!(
                "microphone: the default input {:?}, because {reason}",
                name.unwrap_or("(unknown)")
            );
            default()
        }
        MicChoice::Device { uid, name, reason } => {
            let found = host
                .input_devices()
                .ok()
                .and_then(|mut all| all.find(|d| d.id().is_ok_and(|id| id.id() == uid)));
            match found {
                Some(device) => {
                    tracing::info!("microphone: {name:?} instead of the default, because {reason}");
                    Ok(device)
                }
                None => {
                    tracing::info!(
                        "microphone: the default input; {name:?} was chosen but cpal does not \
                         list it"
                    );
                    default()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TUR-162: a `MicSource` dropped without `stop` (a start that timed
    /// out and finished later, or one a caller let go of) used to leave its
    /// worker polling forever. The drop now joins it. The worker here is the
    /// real one, over a ring nothing ever writes to, as after the stream
    /// stopped.
    #[test]
    fn dropping_a_running_mic_source_ends_its_worker() {
        let dir = tempfile::tempdir().unwrap();
        let track = TrackWriter::open(&dir.path().join("mic.wav"), "microphone").unwrap();
        let rates = FixedRates::new("microphone", 48_000);
        let (_producer, consumer) = HeapRb::<f32>::new(64).split();
        let (_marks, clock) = time_marks(1);
        let worker = Worker::spawn("test-mic-worker", {
            let rates = Arc::clone(&rates);
            let pipeline = Pipeline::new("microphone", 1, 48_000);
            move |running| {
                MicSource::worker_loop(consumer, pipeline, rates, track, clock, running, None)
            }
        })
        .unwrap();
        let mut source = MicSource::new();
        source.worker = Some(worker);
        assert_eq!(Arc::strong_count(&rates), 2, "the worker holds the rates");

        drop(source);
        assert_eq!(
            Arc::strong_count(&rates),
            1,
            "the worker had returned by the time the drop did"
        );
    }
}
