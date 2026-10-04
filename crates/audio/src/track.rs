//! One channel's 16 kHz WAV and the position latched with it, shared by
//! every `cpal`-fed [`crate::AudioSource`]: the microphone ([`crate::mic`]) and
//! the output-device loopback ([`crate::loopback`], TUR-37).
//!
//! Both sources run the same worker-side steps: open the file (or append to
//! it after a segment reopen), append each resampled chunk while latching the
//! host time of the callback that produced it, hand the chunk to the tee, and
//! answer `position`, `fsync_data`, `patch_header` and `pad_leading_silence`
//! from under the same lock. [`TrackWriter`] is those steps, once.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::tee::Tee;
use crate::wav_writer::WavWriter;

/// How long a source's worker thread sleeps when its ring is empty, between
/// polls. Short enough that it never becomes the dominant source of latency
/// against the 200 ms drift gate; long enough not to spin a core.
pub(crate) const IDLE_POLL: Duration = Duration::from_millis(2);

struct Track {
    writer: WavWriter,
    /// Frames written this segment: what [`TrackWriter::position`] reports,
    /// latched together with `last_host_ns` by the same append.
    frames: u64,
    last_host_ns: u64,
}

/// A channel's WAV writer, frame count and latched host time behind one lock.
/// Cheap to clone: the worker appends through one clone while the source
/// answers the orchestrator through another.
#[derive(Clone)]
pub(crate) struct TrackWriter {
    track: Arc<Mutex<Track>>,
    /// What the log calls this channel.
    label: &'static str,
}

impl TrackWriter {
    /// Create `dest`, or append to it when it exists: a segment reopen
    /// (device change) restarts capture against the same file, so the WAV
    /// stays one continuous archive across segments. The frame count still
    /// starts at 0, because positions are segment-relative, while
    /// [`WavWriter::open_append`] carries the file-wide total the header
    /// declares.
    pub(crate) fn open(dest: &Path, label: &'static str) -> io::Result<Self> {
        let writer = if dest.exists() {
            WavWriter::open_append(dest)?
        } else {
            WavWriter::create(dest)?
        };
        Ok(Self {
            track: Arc::new(Mutex::new(Track {
                writer,
                frames: 0,
                last_host_ns: 0,
            })),
            label,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Track> {
        // A panic mid-append leaves the writer as consistent as an I/O error
        // would; keep recording rather than lose the rest of the track.
        self.track.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The worker's sink: append `frames`, latch `host_ns` with the new frame
    /// count, then (with the lock released) offer the chunk to `tee`. A failed
    /// append is logged and the chunk dropped, so the latch never claims
    /// frames the file does not have.
    pub(crate) fn append(&self, frames: &[i16], host_ns: u64, tee: Option<&Tee>) {
        let mut track = self.lock();
        if track.writer.append(frames).is_err() {
            tracing::warn!(
                "{} wav writer append failed; dropping this chunk",
                self.label
            );
            return;
        }
        track.frames += frames.len() as u64;
        track.last_host_ns = host_ns;
        // Released before the tee sees anything: the tee never blocks, but
        // `position()` has no reason to wait on it either way.
        drop(track);
        if let Some(tee) = tee {
            tee.offer(frames);
        }
    }

    /// The latest `(host_ns, frames)` pair, or `None` before the first chunk
    /// with a host time was written.
    pub(crate) fn position(&self) -> Option<(u64, u64)> {
        let track = self.lock();
        (track.last_host_ns != 0).then_some((track.last_host_ns, track.frames))
    }

    pub(crate) fn fsync_data(&self) -> io::Result<()> {
        self.lock().writer.fsync_data()
    }

    pub(crate) fn patch_header(&self) -> io::Result<()> {
        self.lock().writer.patch_header()
    }

    /// Both, under one lock: what a source's `stop` does once its worker
    /// has finished.
    pub(crate) fn finish(&self) -> io::Result<()> {
        let mut track = self.lock();
        track.writer.fsync_data()?;
        track.writer.patch_header()
    }

    /// Contract §6's head-pad. Holding the lock keeps the worker's appends
    /// out of the splice; the tee gets the same pad so both timelines stay
    /// the same length.
    pub(crate) fn pad_leading_silence(&self, frames: u64, tee: Option<&Tee>) -> io::Result<()> {
        let mut track = self.lock();
        track.writer.prepend_silence(frames)?;
        track.frames += frames;
        drop(track);
        if let Some(tee) = tee {
            tee.offer_silence(frames);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames_on_disk(path: &Path) -> u32 {
        hound::WavReader::open(path).unwrap().duration()
    }

    #[test]
    fn position_latches_host_time_with_the_frames_written() {
        let dir = tempfile::tempdir().unwrap();
        let track = TrackWriter::open(&dir.path().join("t.wav"), "test").unwrap();
        assert_eq!(track.position(), None);
        // No host time yet (0): written, but no position to report.
        track.append(&[1; 10], 0, None);
        assert_eq!(track.position(), None);
        track.append(&[1; 5], 42, None);
        assert_eq!(track.position(), Some((42, 15)));
    }

    #[test]
    fn the_tee_sees_every_chunk_and_the_pad() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let track = TrackWriter::open(&path, "test").unwrap();
        let (tee, feed) = crate::tee::tee();
        track.append(&[3; 7], 1, Some(&tee));
        track.pad_leading_silence(4, Some(&tee)).unwrap();
        track.finish().unwrap();
        let mut teed = 0;
        while let Ok(chunk) = feed.try_recv() {
            teed += chunk.len();
        }
        assert_eq!(teed, 11);
        assert_eq!(frames_on_disk(&path), 11);
        assert_eq!(track.position(), Some((1, 11)));
    }

    #[test]
    fn a_reopen_appends_and_counts_from_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let first = TrackWriter::open(&path, "test").unwrap();
        first.append(&[1; 8], 1, None);
        first.finish().unwrap();
        let second = TrackWriter::open(&path, "test").unwrap();
        second.append(&[1; 3], 2, None);
        second.finish().unwrap();
        assert_eq!(second.position(), Some((2, 3)), "segment-relative");
        assert_eq!(frames_on_disk(&path), 11, "one continuous file");
    }

    #[test]
    fn checkpoint_steps_make_the_header_declare_the_frames() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let track = TrackWriter::open(&path, "test").unwrap();
        track.append(&[1; 20], 5, None);
        track.fsync_data().unwrap();
        track.patch_header().unwrap();
        assert_eq!(crate::wav_writer::read_header_frames(&path).unwrap(), 20);
    }
}
