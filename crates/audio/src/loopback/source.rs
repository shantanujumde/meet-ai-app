//! The loopback [`AudioSource`] (TUR-37): an OS [`Backend`]'s packets to a
//! 16 kHz `system.wav`, through the same [`Pipeline`] and [`WavWriter`] as
//! the microphone, with the silence keepalive and gap filling on top.
//!
//! The [`Backend`] is the only OS code: on Windows a `cpal` input stream on
//! the default output device plus a render stream of zeros
//! (`platform/windows_loopback.rs`); in the tests a fake, so the keepalive's
//! lifecycle and the gap filling are checked on every OS.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use ringbuf::traits::{Consumer, Split};
use ringbuf::{HeapCons, HeapRb};

use super::capture::{Capture, CaptureStats};
use super::clock::{frames_to_ns, ns_to_frames};
use super::splice::{GapMark, Piece, splice};
use crate::pipeline::Pipeline;
use crate::rate_meter::{FixedRates, Rates};
use crate::segments::SAMPLE_RATE_HZ;
use crate::tee::Tee;
use crate::wav_writer::WavWriter;
use crate::{AudioSource, Channel, Error};

/// What the log calls this channel.
const LABEL: &str = "system loopback";

/// Seconds of device audio the sample ring holds for a late worker.
const RING_SECONDS: usize = 4;

/// Gap marks the callback can queue before the worker takes them.
const MARK_CAPACITY: usize = 256;

/// How long the worker sleeps when there is nothing to do.
const IDLE_POLL: Duration = Duration::from_millis(2);

/// Samples the worker takes from the ring at a time.
const SCRATCH_SAMPLES: usize = 4096;

/// The start of every gap goes through the resampler as zeros, so the
/// audio before it (and the filter's tail) comes out first and in order;
/// the rest is written as 16 kHz silence directly, so a gap of minutes costs
/// no resampling. Two resampler chunks at least.
const FLUSH_THROUGH_MS: u64 = 100;
const FLUSH_THROUGH_MIN_FRAMES: u64 = 2048;

/// A loopback device's stream format, as the [`Backend`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Format {
    /// Frames per second the device delivers.
    pub rate: u32,
    /// Interleaved channels per frame.
    pub channels: usize,
    /// The device's name, for the log.
    pub label: String,
}

/// A running OS stream. Dropping it must stop it too.
pub trait LiveStream: Send {
    /// Stop delivering (or rendering).
    fn pause(&mut self);
}

/// The OS half of a loopback source.
pub trait Backend: Send + 'static {
    /// The format of the device to capture, read fresh at every start.
    fn format(&mut self) -> Result<Format, Error>;

    /// Start rendering silence to that device, so its loopback keeps sending
    /// packets while nothing else plays. Failure is not fatal: capture then
    /// only gets packets while something plays, and the gaps become silence.
    fn start_keepalive(&mut self) -> Result<Box<dyn LiveStream>, Error>;

    /// Start capturing in `format`, calling [`Capture::packet`] from the
    /// capture callback for every packet.
    fn start_capture(
        &mut self,
        format: &Format,
        capture: Capture,
    ) -> Result<Box<dyn LiveStream>, Error>;
}

/// What the worker writes and [`AudioSource::position`] reads.
struct Shared {
    writer: WavWriter,
    /// Frames written this segment, latched with `last_host_ns`.
    frames: u64,
    last_host_ns: u64,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // A panic mid-append leaves the writer as consistent as an I/O error
    // would; keep recording rather than lose the rest of the track.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// System audio from an output device's loopback, as an [`AudioSource`].
pub struct LoopbackSource<B: Backend> {
    backend: B,
    capture: Option<Box<dyn LiveStream>>,
    keepalive: Option<Box<dyn LiveStream>>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    shared: Option<Arc<Mutex<Shared>>>,
    tee: Option<Tee>,
    rates: Option<Arc<FixedRates>>,
}

impl<B: Backend> LoopbackSource<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            capture: None,
            keepalive: None,
            worker: None,
            running: Arc::new(AtomicBool::new(false)),
            shared: None,
            tee: None,
            rates: None,
        }
    }

    /// Whether the silence keepalive is running (it may have failed to start).
    pub fn has_keepalive(&self) -> bool {
        self.keepalive.is_some()
    }

    /// Stop both streams and the worker; the writer stays for the header.
    fn halt(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(mut stream) = self.capture.take() {
            stream.pause();
        }
        if let Some(mut keepalive) = self.keepalive.take() {
            keepalive.pause();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl<B: Backend> Drop for LoopbackSource<B> {
    fn drop(&mut self) {
        // Never leave the worker polling an abandoned ring.
        self.halt();
    }
}

impl<B: Backend> AudioSource for LoopbackSource<B> {
    fn start(&mut self, dest: PathBuf) -> Result<(), Error> {
        self.halt();
        let format = self.backend.format()?;
        if format.rate == 0 || format.channels == 0 {
            return Err(Error::NoDevice(format!(
                "{} reports {} Hz and {} channels",
                format.label, format.rate, format.channels
            )));
        }
        tracing::info!(
            "{LABEL}: {} at {} Hz, {} ch",
            format.label,
            format.rate,
            format.channels
        );

        // Same as the microphone: a reopened segment appends to the same file.
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
        let rates = FixedRates::new(LABEL, format.rate);
        let ring = RING_SECONDS * format.rate as usize * format.channels;
        let (producer, consumer) = HeapRb::<f32>::new(ring).split();
        let (mark_tx, marks) = HeapRb::<GapMark>::new(MARK_CAPACITY).split();
        let last_ns = Arc::new(AtomicU64::new(0));
        let stats = Arc::new(CaptureStats::default());

        // Before the capture, so its first packet does not wait for a sound.
        let keepalive = match self.backend.start_keepalive() {
            Ok(keepalive) => {
                tracing::info!("{LABEL}: silence keepalive running");
                Some(keepalive)
            }
            Err(e) => {
                tracing::warn!(
                    "{LABEL}: no silence keepalive ({e}); the device sends nothing while \
                     nothing plays, and those stretches are filled with silence"
                );
                None
            }
        };
        let capture = Capture::new(
            producer,
            mark_tx,
            Arc::clone(&rates),
            Arc::clone(&last_ns),
            format.channels,
            Arc::clone(&stats),
        );
        let stream = match self.backend.start_capture(&format, capture) {
            Ok(stream) => stream,
            Err(e) => {
                if let Some(mut keepalive) = keepalive {
                    keepalive.pause();
                }
                return Err(e);
            }
        };

        let running = Arc::new(AtomicBool::new(true));
        let worker = Worker {
            consumer,
            marks,
            pipeline: Pipeline::new(LABEL, format.channels, format.rate),
            rates: Arc::clone(&rates),
            shared: Arc::clone(&shared),
            last_ns,
            running: Arc::clone(&running),
            tee: self.tee.clone(),
            stats,
            channels: format.channels,
        };
        let spawned = std::thread::Builder::new()
            .name("meet-rec-loopback-worker".to_string())
            .spawn(move || worker.run());
        let handle = match spawned {
            Ok(handle) => handle,
            Err(e) => {
                let mut stream = stream;
                stream.pause();
                if let Some(mut keepalive) = keepalive {
                    keepalive.pause();
                }
                return Err(Error::Io(e));
            }
        };

        self.capture = Some(stream);
        self.keepalive = keepalive;
        self.worker = Some(handle);
        self.running = running;
        self.shared = Some(shared);
        self.rates = Some(rates);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.halt();
        if let Some(shared) = &self.shared {
            let mut guard = lock(shared);
            guard.writer.fsync_data()?;
            guard.writer.patch_header()?;
        }
        Ok(())
    }

    fn channel(&self) -> Channel {
        Channel::System
    }

    fn position(&self) -> Option<(u64, u64)> {
        let guard = lock(self.shared.as_ref()?);
        (guard.last_host_ns != 0).then_some((guard.last_host_ns, guard.frames))
    }

    fn fsync_data(&mut self) -> Result<(), Error> {
        if let Some(shared) = &self.shared {
            lock(shared).writer.fsync_data()?;
        }
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), Error> {
        if let Some(shared) = &self.shared {
            lock(shared).writer.patch_header()?;
        }
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), Error> {
        let Some(shared) = &self.shared else {
            return Ok(());
        };
        // Holding the lock keeps the worker's appends out of the splice.
        let mut guard = lock(shared);
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

    fn rate_report(&self) -> Option<String> {
        self.rates.as_ref().map(|rates| rates.describe())
    }

    fn device_rate(&self) -> Option<u32> {
        self.rates.as_ref().map(|rates| rates.effective())
    }
}

/// The worker thread: ring to [`Pipeline`] to [`WavWriter`], with the gap
/// marks spliced in as silence.
struct Worker {
    consumer: HeapCons<f32>,
    marks: HeapCons<GapMark>,
    pipeline: Pipeline,
    rates: Arc<FixedRates>,
    shared: Arc<Mutex<Shared>>,
    last_ns: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    tee: Option<Tee>,
    stats: Arc<CaptureStats>,
    channels: usize,
}

impl Worker {
    fn run(self) {
        let Worker {
            mut consumer,
            mut marks,
            mut pipeline,
            rates,
            shared,
            last_ns,
            running,
            tee,
            stats,
            channels,
        } = self;
        let mut sink = |frames: &[i16]| {
            let host_ns = last_ns.load(Ordering::Acquire);
            let mut guard = lock(&shared);
            if guard.writer.append(frames).is_err() {
                tracing::warn!("{LABEL} wav writer append failed; dropping this chunk");
                return;
            }
            guard.frames += frames.len() as u64;
            guard.last_host_ns = host_ns;
            drop(guard);
            if let Some(tee) = &tee {
                tee.offer(frames);
            }
        };
        let mut silence = Silence::new(channels);
        let mut pending: VecDeque<GapMark> = VecDeque::new();
        let mut consumed: u64 = 0;
        let mut scratch = vec![0.0f32; SCRATCH_SAMPLES];
        loop {
            // Samples first, marks second: a mark is pushed before the
            // samples after it, so every mark these samples need is here.
            let popped = consumer.pop_slice(&mut scratch);
            pending.extend(marks.pop_iter());
            let rate = rates.effective();
            if popped > 0 {
                pipeline.follow(&*rates, &mut sink);
            }
            splice(
                consumed,
                &scratch[..popped],
                &mut pending,
                |piece| match piece {
                    Piece::Samples(samples) => pipeline.push(samples, &mut sink),
                    Piece::Silence(frames) => {
                        tracing::info!(
                            "{LABEL}: {} ms the device did not deliver, written as silence",
                            frames_to_ns(frames, rate) / 1_000_000
                        );
                        silence.write(frames, rate, &mut pipeline, &mut sink);
                    }
                },
            );
            consumed += popped as u64;
            if popped == 0 {
                if !running.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(IDLE_POLL);
            }
        }
        tracing::info!("{LABEL} stopped: {}", stats.describe());
    }
}

/// Writes gap silence: the first part through the resampler, the rest
/// straight to the sink at 16 kHz.
struct Silence {
    channels: usize,
    /// Device-rate zeros, a whole number of frames long.
    device: Vec<f32>,
    /// 16 kHz zeros.
    output: Vec<i16>,
}

impl Silence {
    fn new(channels: usize) -> Self {
        let channels = channels.max(1);
        Self {
            channels,
            device: vec![0.0; (SCRATCH_SAMPLES / channels).max(1) * channels],
            output: vec![0; SCRATCH_SAMPLES],
        }
    }

    fn write(
        &mut self,
        frames: u64,
        rate: u32,
        pipeline: &mut Pipeline,
        sink: &mut impl FnMut(&[i16]),
    ) {
        let flush = (u64::from(rate) * FLUSH_THROUGH_MS / 1000).max(FLUSH_THROUGH_MIN_FRAMES);
        let through = frames.min(flush);
        let mut samples = through as usize * self.channels;
        while samples > 0 {
            let n = samples.min(self.device.len());
            pipeline.push(&self.device[..n], sink);
            samples -= n;
        }
        let rest = frames - through;
        let mut out = ns_to_frames(frames_to_ns(rest, rate), SAMPLE_RATE_HZ) as usize;
        while out > 0 {
            let n = out.min(self.output.len());
            sink(&self.output[..n]);
            out -= n;
        }
    }
}

#[cfg(test)]
mod tests;
