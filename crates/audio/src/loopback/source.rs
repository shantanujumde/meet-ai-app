//! The loopback [`AudioSource`] (TUR-37): an OS [`Backend`]'s packets to a
//! 16 kHz `system.wav`, through the same [`Pipeline`] and [`TrackWriter`] as
//! the microphone, with the silence keepalive and gap filling on top.
//!
//! The [`Backend`] is the only OS code: on Windows a `cpal` input stream on
//! the default output device plus a render stream of zeros
//! (`platform/windows_loopback.rs`); in the tests a fake, so the keepalive's
//! lifecycle and the gap filling are checked on every OS.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use ringbuf::traits::{Consumer, Split};
use ringbuf::{HeapCons, HeapRb};

use super::capture::{Capture, CaptureStats};
use super::clock::{GapRule, frames_to_ns, ns_to_frames};
use super::splice::{GapMark, Piece, splice};
use crate::capture_clock::{CaptureClock, Feed, time_marks};
use crate::pipeline::Pipeline;
use crate::rate_meter::{FixedRates, Rates};
use crate::segments::SAMPLE_RATE_HZ;
use crate::tee::Tee;
use crate::track::{IDLE_POLL, TrackWriter};
use crate::{AudioSource, Channel, Error};

/// What the log calls this channel.
const LABEL: &str = "system loopback";

/// Seconds of device audio the sample ring holds for a late worker.
const RING_SECONDS: usize = 4;

/// Gap marks the callback can queue before the worker takes them.
const MARK_CAPACITY: usize = 256;

/// Samples the worker takes from the ring at a time, and the length of its
/// buffers of zeros for gap silence.
const WORKER_POP_SAMPLES: usize = 4096;

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

    /// How late a packet must start to be a gap (TUR-38), read after
    /// [`Self::format`]. TUR-37's rule unless the backend knows its times
    /// better (PipeWire) or worse (PulseAudio).
    fn gap_rule(&self) -> GapRule {
        GapRule::WASAPI
    }
}

/// Pause every stream a failed start had opened, before the error goes back.
fn abandon(streams: impl IntoIterator<Item = Box<dyn LiveStream>>) {
    for mut stream in streams {
        stream.pause();
    }
}

/// System audio from an output device's loopback, as an [`AudioSource`].
pub struct LoopbackSource<B: Backend> {
    backend: B,
    capture: Option<Box<dyn LiveStream>>,
    keepalive: Option<Box<dyn LiveStream>>,
    worker: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    track: Option<TrackWriter>,
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
            track: None,
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
        let track = TrackWriter::open(&dest, LABEL)?;
        let rates = FixedRates::new(LABEL, format.rate);
        let ring = RING_SECONDS * format.rate as usize * format.channels;
        let (producer, consumer) = HeapRb::<f32>::new(ring).split();
        let (mark_tx, marks) = HeapRb::<GapMark>::new(MARK_CAPACITY).split();
        // Each packet's capture time rides next to its samples (TUR-151).
        let (times, clock) = time_marks(format.channels);
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
            times,
            format.channels,
            Arc::clone(&stats),
        )
        .with_gap_rule(self.backend.gap_rule());
        let stream = match self.backend.start_capture(&format, capture) {
            Ok(stream) => stream,
            Err(e) => {
                abandon(keepalive);
                return Err(e);
            }
        };

        let running = Arc::new(AtomicBool::new(true));
        let worker = Worker {
            consumer,
            marks,
            pipeline: Pipeline::new(LABEL, format.channels, format.rate),
            rates: Arc::clone(&rates),
            track: track.clone(),
            clock,
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
                abandon(std::iter::once(stream).chain(keepalive));
                return Err(Error::Io(e));
            }
        };

        self.capture = Some(stream);
        self.keepalive = keepalive;
        self.worker = Some(handle);
        self.running = running;
        self.track = Some(track);
        self.rates = Some(rates);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.halt();
        if let Some(track) = &self.track {
            track.finish()?;
        }
        Ok(())
    }

    fn channel(&self) -> Channel {
        Channel::System
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
        // The track's lock keeps the worker's appends out of the splice.
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

/// The worker thread: ring to [`Pipeline`] to [`TrackWriter`], with the gap
/// marks spliced in as silence.
struct Worker {
    consumer: HeapCons<f32>,
    marks: HeapCons<GapMark>,
    pipeline: Pipeline,
    rates: Arc<FixedRates>,
    track: TrackWriter,
    clock: CaptureClock,
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
            track,
            clock,
            running,
            tee,
            stats,
            channels,
        } = self;
        // Every append timed by the capture of the frame it ends at (TUR-151).
        let mut feed = Feed::new(track, clock, tee);
        let mut silence = Silence::new(channels);
        let mut pending: VecDeque<GapMark> = VecDeque::new();
        let mut consumed: u64 = 0;
        let mut scratch = vec![0.0f32; WORKER_POP_SAMPLES];
        loop {
            // Samples first, marks second: a mark is pushed before the
            // samples after it, so every mark these samples need is here.
            let popped = consumer.pop_slice(&mut scratch);
            pending.extend(marks.pop_iter());
            let rate = rates.effective();
            if popped > 0 {
                pipeline.follow(&*rates, &mut |f: &[i16]| feed.extend(f));
            }
            splice(
                consumed,
                &scratch[..popped],
                &mut pending,
                |piece| match piece {
                    Piece::Samples(samples) => {
                        pipeline.push(samples, &mut |f: &[i16]| feed.extend(f));
                        feed.consumed(samples.len());
                    }
                    Piece::Silence(frames) => {
                        tracing::info!(
                            "{LABEL}: {} ms the device did not deliver, written as silence",
                            frames_to_ns(frames, rate) / 1_000_000
                        );
                        let rest =
                            silence.write(frames, rate, &mut pipeline, &mut |f: &[i16]| {
                                feed.extend(f)
                            });
                        feed.silence(rest, &pipeline);
                    }
                },
            );
            feed.flush(&pipeline);
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

/// Writes gap silence: the first part through the resampler, and says how
/// much of the rest is owed straight at 16 kHz ([`Feed::silence`] writes it,
/// timed, TUR-151).
struct Silence {
    channels: usize,
    /// Device-rate zeros, a whole number of frames long.
    device: Vec<f32>,
}

impl Silence {
    fn new(channels: usize) -> Self {
        let channels = channels.max(1);
        Self {
            channels,
            device: vec![0.0; (WORKER_POP_SAMPLES / channels).max(1) * channels],
        }
    }

    /// Push the start of a `frames`-long gap through `pipeline`; returns the
    /// 16 kHz frames of silence still owed for the rest of it.
    fn write(
        &mut self,
        frames: u64,
        rate: u32,
        pipeline: &mut Pipeline,
        sink: &mut impl FnMut(&[i16]),
    ) -> u64 {
        let flush = (u64::from(rate) * FLUSH_THROUGH_MS / 1000).max(FLUSH_THROUGH_MIN_FRAMES);
        let through = frames.min(flush);
        let mut samples = through as usize * self.channels;
        while samples > 0 {
            let n = samples.min(self.device.len());
            pipeline.push(&self.device[..n], sink);
            samples -= n;
        }
        let rest = frames - through;
        ns_to_frames(frames_to_ns(rest, rate), SAMPLE_RATE_HZ)
    }
}

#[cfg(test)]
mod tests;
