//! Microphone capture via `cpal` (SPEC §2.3).
//!
//! `cpal` is already cross-platform (SETUP.md: pinned at 0.18.2 for exactly
//! this reason), so unlike the process tap — macOS-only, lives in
//! [`crate::macos`] per SPEC §4's ⛔ — this module needs no platform gate of
//! its own. The one exception is the host-clock read each callback stamps
//! itself with, isolated in [`host_now_ns`] below.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};

use crate::resample::{Resampler, downmix_to_mono};
use crate::tee::Tee;
use crate::wav_writer::WavWriter;
use crate::{AudioSource, Channel, Error};

/// Host time "now", in the same clock domain as the process tap's
/// `AudioTimeStamp.mHostTime` converted via `AudioConvertHostTimeToNanos`
/// (contract revision 3, §0: "mach_absolute_time, converted to ns via
/// mach_timebase_info"). `AudioGetCurrentHostTime` returns exactly the value
/// `mach_absolute_time()` would — same underlying counter — so calling it
/// from the mic callback keeps both channels' anchors comparable without
/// needing to reconcile two different clock epochs. `cpal`'s own per-callback
/// `InputCallbackInfo` timestamp is not documented to share that domain, so
/// this reads the Core Audio host clock directly instead of trusting it.
#[cfg(target_os = "macos")]
fn host_now_ns() -> u64 {
    // SAFETY: both calls read current host-clock state; neither takes a
    // pointer or has a precondition beyond "the audio HAL is initialized",
    // which it is by the time any cpal stream callback can run.
    unsafe {
        objc2_core_audio::AudioConvertHostTimeToNanos(objc2_core_audio::AudioGetCurrentHostTime())
    }
}

/// SPEC §8.2's Windows port is a stub, not held to the drift gate, so a
/// process-relative monotonic clock is sufficient here — there is no second
/// channel on that platform yet for it to be compared against.
#[cfg(not(target_os = "macos"))]
fn host_now_ns() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Ring buffer capacity, in raw (device-rate, interleaved) samples. 4 s at
/// 48 kHz stereo is generously oversized for a worker thread serviced every
/// couple of milliseconds; it exists so a scheduling hiccup drops nothing
/// rather than to hold steady-state backlog.
const RING_CAPACITY_SAMPLES: usize = 48_000 * 2 * 4;

/// How long the worker thread sleeps when the ring buffer is empty, between
/// polls. Short enough that it never becomes the dominant source of latency
/// against the 200 ms drift gate; long enough not to spin a core.
const IDLE_POLL: Duration = Duration::from_millis(2);

struct Shared {
    writer: WavWriter,
    /// Frames written to `writer` so far — the same count [`AudioSource::position`]
    /// reports, latched together with `last_host_ns` from the same write.
    frames: u64,
    last_host_ns: u64,
}

/// Everything [`MicSource::start`] needs to hand back once the stream is
/// actually live.
struct Built {
    stream: Stream,
    worker: JoinHandle<()>,
    running: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// The microphone capture channel: a `cpal` input stream feeding a resampler
/// and a [`WavWriter`] through a lock-free ring buffer, per SPEC §2.3's
/// requirement that the real-time audio callback never blocks, allocates, or
/// touches the filesystem.
pub struct MicSource {
    stream: Option<Stream>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    shared: Option<Arc<Mutex<Shared>>>,
    /// The live-transcription copy, if one was asked for ([`AudioSource::tee`]).
    tee: Option<Tee>,
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
            shared: None,
            tee: None,
        }
    }

    fn worker_loop(
        mut consumer: HeapCons<f32>,
        channels: usize,
        device_rate: u32,
        shared: Arc<Mutex<Shared>>,
        last_cb_host_ns: Arc<AtomicU64>,
        running: Arc<AtomicBool>,
        tee: Option<Tee>,
    ) {
        let mut resampler = Resampler::new(device_rate);
        let chunk_raw_len = resampler.input_chunk_frames() * channels.max(1);

        let mut pending: Vec<f32> = Vec::with_capacity(chunk_raw_len * 2);
        let mut scratch = vec![0.0f32; 4096];
        let mut mono = Vec::with_capacity(resampler.input_chunk_frames());
        let mut i16_buf: Vec<i16> = Vec::with_capacity(resampler.input_chunk_frames());

        loop {
            let popped = consumer.pop_slice(&mut scratch);
            if popped > 0 {
                pending.extend_from_slice(&scratch[..popped]);
            } else if !running.load(Ordering::Acquire) {
                break;
            } else {
                std::thread::sleep(IDLE_POLL);
                continue;
            }

            while pending.len() >= chunk_raw_len {
                let raw: Vec<f32> = pending.drain(..chunk_raw_len).collect();
                downmix_to_mono(&raw, channels.max(1), &mut mono);
                let out = resampler.process(&mono);
                if out.is_empty() {
                    continue;
                }
                i16_buf.clear();
                i16_buf.extend(out.iter().copied().map(f32_to_i16));
                let host_ns = last_cb_host_ns.load(Ordering::Relaxed);

                let mut guard = shared.lock().expect("mic writer mutex poisoned");
                if guard.writer.append(&i16_buf).is_err() {
                    tracing::warn!("mic wav writer append failed; dropping this chunk");
                    continue;
                }
                guard.frames += i16_buf.len() as u64;
                guard.last_host_ns = host_ns;
                // Released before the tee sees anything: the tee never blocks,
                // but `position()` has no reason to wait on it either way.
                drop(guard);
                if let Some(tee) = &tee {
                    tee.offer(&i16_buf);
                }
            }
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
        let device = host
            .default_input_device()
            .ok_or_else(|| Error::NoDevice("no default input device".to_string()))?;
        let supported = device
            .default_input_config()
            .map_err(|e| Error::NoDevice(format!("no usable input config: {e}")))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let device_rate = config.sample_rate;
        let channels = config.channels as usize;

        // A segment reopen (device change) restarts capture against the same
        // `dest` a previous `MicSource` already wrote to — the WAV stays one
        // continuous per-channel archive across segments (contract:
        // "concatenated in idx order"), only the OS-level stream is rebuilt.
        // `Shared::frames` still starts at 0 here regardless: it is
        // segment-relative (`crate::segments`'s per-segment frame counts),
        // while `WavWriter::open_append` is what carries the file-wide,
        // cross-segment total the header must keep declaring.
        let writer = if dest.exists() {
            WavWriter::open_append(&dest)?
        } else {
            WavWriter::create(&dest)?
        };
        let shared = Arc::new(Mutex::new(Shared {
            writer,
            frames: 0,
            last_host_ns: 0,
        }));

        let rb = HeapRb::<f32>::new(RING_CAPACITY_SAMPLES);
        let (mut producer, consumer) = rb.split();
        let last_cb_host_ns = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));

        let cb_host_ns_for_stream = Arc::clone(&last_cb_host_ns);
        let err_fn = |err| tracing::warn!("cpal input stream error: {err}");

        // The callback itself: timestamp, then push into the lock-free ring.
        // No allocation, no lock, no I/O — SPEC §2.3's real-time constraint.
        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _| {
                    cb_host_ns_for_stream.store(host_now_ns(), Ordering::Relaxed);
                    let _ = producer.push_slice(data);
                },
                err_fn,
                Some(crate::AUDIO_PERMISSION_TIMEOUT),
            ),
            SampleFormat::I16 => {
                let mut scratch = vec![0.0f32; 8192];
                device.build_input_stream(
                    config,
                    move |data: &[i16], _| {
                        cb_host_ns_for_stream.store(host_now_ns(), Ordering::Relaxed);
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
                let shared = Arc::clone(&shared);
                let running = Arc::clone(&running);
                move || {
                    Self::worker_loop(
                        consumer,
                        channels,
                        device_rate,
                        shared,
                        last_cb_host_ns,
                        running,
                        tee,
                    )
                }
            })
            .expect("spawning the mic worker thread");

        Ok(Built {
            stream,
            worker,
            running,
            shared,
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
        self.worker = Some(built.worker);
        self.running = built.running;
        self.shared = Some(built.shared);
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
        if let Some(shared) = &self.shared {
            let mut guard = shared.lock().expect("mic writer mutex poisoned");
            guard.writer.fsync_data()?;
            guard.writer.patch_header()?;
        }
        Ok(())
    }

    fn channel(&self) -> Channel {
        Channel::Mic
    }

    fn position(&self) -> Option<(u64, u64)> {
        let shared = self.shared.as_ref()?;
        let guard = shared.lock().expect("mic writer mutex poisoned");
        if guard.last_host_ns == 0 {
            None
        } else {
            Some((guard.last_host_ns, guard.frames))
        }
    }

    fn fsync_data(&mut self) -> Result<(), Error> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        let mut guard = shared.lock().expect("mic writer mutex poisoned");
        guard.writer.fsync_data()?;
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), Error> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        let mut guard = shared.lock().expect("mic writer mutex poisoned");
        guard.writer.patch_header()?;
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), Error> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        // Locking here excludes the worker thread's own `shared.lock()` in
        // `worker_loop` for the duration of the splice, so no append can land
        // between our read-the-tail and write-the-pad steps.
        let mut guard = shared.lock().expect("mic writer mutex poisoned");
        guard.writer.prepend_silence(frames)?;
        guard.frames += frames;
        drop(guard);
        if let Some(tee) = &self.tee {
            tee.offer_silence(frames);
        }
        Ok(())
    }

    fn tee(&mut self, tee: Tee) {
        self.tee = Some(tee);
    }
}
