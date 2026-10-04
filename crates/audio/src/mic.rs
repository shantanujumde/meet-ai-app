//! Microphone capture via `cpal` (SPEC §2.3).
//!
//! `cpal` is already cross-platform (SETUP.md: pinned at 0.18.2 for exactly
//! this reason), so unlike the process tap — macOS-only, lives in
//! [`crate::macos`] per SPEC §4's ⛔ — this module needs no platform gate of
//! its own. The one exception is the host-clock read each callback stamps
//! itself with, which is OS code and comes from `crate::platform`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};

use crate::mic_choice::{self, MicChoice};
use crate::pipeline::Pipeline;
use crate::platform::host_now_ns;
// TUR-37: on Windows the capture time WASAPI stamps, the loopback's clock too.
use crate::platform::input_callback_ns;
use crate::rate_meter::{CallbackMeter, FixedRates, Rates};
use crate::tee::Tee;
use crate::track::{IDLE_POLL, TrackWriter};
use crate::{AudioSource, Channel, Error};

/// Ring buffer capacity, in raw (device-rate, interleaved) samples. 4 s at
/// 48 kHz stereo is generously oversized for a worker thread serviced every
/// couple of milliseconds; it exists so a scheduling hiccup drops nothing
/// rather than to hold steady-state backlog.
const RING_CAPACITY_SAMPLES: usize = 48_000 * 2 * 4;

/// Everything [`MicSource::start`] needs to hand back once the stream is
/// actually live.
struct Built {
    stream: Stream,
    rates: Arc<FixedRates>,
    worker: JoinHandle<()>,
    running: Arc<AtomicBool>,
    track: TrackWriter,
}

/// The microphone capture channel: a `cpal` input stream feeding a resampler
/// and a [`crate::wav_writer::WavWriter`] through a lock-free ring buffer, per SPEC §2.3's
/// requirement that the real-time audio callback never blocks, allocates, or
/// touches the filesystem.
pub struct MicSource {
    stream: Option<Stream>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
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
            running: Arc::new(AtomicBool::new(false)),
            track: None,
            tee: None,
            rates: None,
        }
    }

    /// Raw device samples → [`Pipeline`], following the measured rate
    /// (TUR-87): when the callbacks deliver another rate than `cpal`
    /// reported, the resampler is rebuilt at the measured one.
    fn worker_loop(
        mut consumer: HeapCons<f32>,
        mut pipeline: Pipeline,
        rates: Arc<FixedRates>,
        track: TrackWriter,
        last_cb_host_ns: Arc<AtomicU64>,
        running: Arc<AtomicBool>,
        tee: Option<Tee>,
    ) {
        let mut sink = |frames: &[i16]| {
            track.append(
                frames,
                last_cb_host_ns.load(Ordering::Relaxed),
                tee.as_ref(),
            );
        };
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
            pipeline.follow(&*rates, &mut sink);
            pipeline.push(&scratch[..popped], &mut sink);
        }
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
        let config: StreamConfig = supported.into();
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
        let last_cb_host_ns = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));

        let cb_host_ns_for_stream = Arc::clone(&last_cb_host_ns);
        // The delivered-rate meter (TUR-84's, TUR-87): integers and atomics.
        let mut meter = CallbackMeter::new(Arc::clone(&rates), channels);
        let err_fn = |err| tracing::warn!("cpal input stream error: {err}");

        // The callback itself: timestamp, then push into the lock-free ring.
        // No allocation, no lock, no I/O — SPEC §2.3's real-time constraint.
        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], info: &cpal::InputCallbackInfo| {
                    let now = input_callback_ns(info).unwrap_or_else(host_now_ns);
                    cb_host_ns_for_stream.store(now, Ordering::Relaxed);
                    let _ = producer.push_slice(data);
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
                        let now = input_callback_ns(info).unwrap_or_else(host_now_ns);
                        cb_host_ns_for_stream.store(now, Ordering::Relaxed);
                        meter.observe(now, data.len());
                        if scratch.len() < data.len() {
                            scratch.resize(data.len(), 0.0);
                        }
                        for (dst, src) in scratch.iter_mut().zip(data.iter()) {
                            *dst = *src as f32 / i16::MAX as f32;
                        }
                        let _ = producer.push_slice(&scratch[..data.len()]);
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

        let worker = std::thread::Builder::new()
            .name("meet-rec-mic-worker".to_string())
            .spawn({
                let track = track.clone();
                let running = Arc::clone(&running);
                let rates = Arc::clone(&rates);
                let pipeline = Pipeline::new("microphone", channels, device_rate);
                move || {
                    Self::worker_loop(
                        consumer,
                        pipeline,
                        rates,
                        track,
                        last_cb_host_ns,
                        running,
                        tee,
                    )
                }
            })
            .expect("spawning the mic worker thread");

        Ok(Built {
            stream,
            rates,
            worker,
            running,
            track,
        })
    }
}

impl AudioSource for MicSource {
    fn start(&mut self, dest: PathBuf) -> Result<(), Error> {
        let (tx, rx) = std::sync::mpsc::channel();
        let tee = self.tee.clone();
        std::thread::Builder::new()
            .name("meet-rec-mic-init".to_string())
            .spawn(move || {
                let _ = tx.send(Self::build(dest, tee));
            })
            .expect("spawning the mic init thread");

        let built = match rx.recv_timeout(crate::AUDIO_PERMISSION_TIMEOUT) {
            Ok(result) => result?,
            Err(_) => {
                // The init thread is still blocked inside Core Audio and has
                // no way to be cancelled — see `MicSource::build`'s doc. It
                // is deliberately leaked here rather than joined: the
                // alternative is this method hanging with it, which is
                // exactly the failure this timeout exists to turn into a
                // reportable error.
                return Err(Error::PermissionDenied);
            }
        };

        self.stream = Some(built.stream);
        self.rates = Some(built.rates);
        self.worker = Some(built.worker);
        self.running = built.running;
        self.track = Some(built.track);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.running.store(false, Ordering::Release);
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(track) = &self.track {
            track.finish()?;
        }
        Ok(())
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

/// The input device to open: the default, unless [`mic_choice::choose`]
/// picks another (TUR-91: the Mac's own mic when the default is Bluetooth).
/// The choice and its reason are logged; any failure to list or find the
/// chosen device falls back to the default, as before.
fn pick_device(host: &cpal::Host) -> Result<cpal::Device, Error> {
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
