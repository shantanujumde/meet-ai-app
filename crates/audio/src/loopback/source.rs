//! The loopback [`AudioSource`] (TUR-37): an OS [`Backend`]'s packets to a
//! 16 kHz `system.wav`, through the same [`Pipeline`] and [`TrackWriter`] as
//! the microphone, with the silence keepalive and gap filling on top.
//!
//! The [`Backend`] is the only OS code: on Windows a `cpal` input stream on
//! the default output device plus a render stream of zeros
//! (`platform/windows_loopback.rs`); in the tests a fake, so the keepalive's
//! lifecycle and the gap filling are checked on every OS.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use super::capture::Capture;
use super::clock::GapRule;
use super::drain::ring;
use crate::rate_meter::{FixedRates, Rates};
use crate::tee::Tee;
use crate::track::TrackWriter;
use crate::{AudioSource, Channel, Error};

/// What the log calls this channel.
const LABEL: &str = "system loopback";

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
        let (capture, drain) = ring(
            LABEL,
            Arc::clone(&rates),
            format.channels,
            track.clone(),
            self.tee.clone(),
        );

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
        let capture = capture.with_gap_rule(self.backend.gap_rule());
        let stream = match self.backend.start_capture(&format, capture) {
            Ok(stream) => stream,
            Err(e) => {
                abandon(keepalive);
                return Err(e);
            }
        };

        let running = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&running);
        let spawned = std::thread::Builder::new()
            .name("meet-rec-loopback-worker".to_string())
            .spawn(move || drain.run(&flag, || {}));
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
        self.stop_capture()?;
        self.patch_header()
    }

    fn stop_capture(&mut self) -> Result<(), Error> {
        self.halt();
        self.fsync_data()
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

    /// With no keepalive the device sends nothing while nothing plays.
    fn delivers_continuously(&self) -> bool {
        self.keepalive.is_some()
    }
}

#[cfg(test)]
mod tests;
