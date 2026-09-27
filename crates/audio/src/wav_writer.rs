//! Crash-safe incremental WAV writer (contract §7, §13).
//!
//! Writes exactly a canonical 44-byte RIFF/WAVE header — mono 16-bit PCM,
//! `data` as the final chunk, nothing (`LIST`, `fact`, `INFO`) ever inserted
//! between header and samples. That shape is promised to Vox in §13 so a live
//! reader can tail the file at a fixed byte offset (`44 + 2 * frame`) instead
//! of trusting the declared length, which lags by up to one checkpoint.
//!
//! §7's checkpoint order is three steps, and this module owns two of them —
//! the middle step (`segments.json`) belongs to [`crate::segments`], and the
//! caller sequences the three:
//!
//! 1. [`WavWriter::append`] the pending samples, then [`WavWriter::fsync_data`].
//! 2. the caller writes `segments.json` (temp file + `fsync` + `rename(2)`).
//! 3. [`WavWriter::patch_header`], which fsyncs the header region itself.
//!
//! That ordering is what makes the crash invariant hold in the direction that
//! is safe to lose: a `kill -9` between 1 and 2 leaves `segments.json`
//! claiming frames the header does not expose yet, and nobody ever asks about
//! them (`crate::segments::Segments::check_wav_header`). A `kill -9` inside
//! [`WavWriter::patch_header`] itself is handled the same way, one level
//! down: the two header length fields are written RIFF-size first, then
//! data-size — so a torn patch leaves `data`'s declared length at its old,
//! smaller, fully-durable value rather than a larger one nothing backs.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::segments::SAMPLE_RATE_HZ;

const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;
const BYTES_PER_SAMPLE: u32 = (BITS_PER_SAMPLE / 8) as u32;
const HEADER_LEN: u64 = 44;

/// Offset of the RIFF chunk's size field (bytes 4..8): `36 + data_len`.
const RIFF_SIZE_OFFSET: u64 = 4;
/// Offset of the `data` chunk's size field (bytes 40..44): `frames * 2`.
const DATA_SIZE_OFFSET: u64 = 40;

/// An incrementally-written, crash-safe mono 16-bit PCM WAV file.
///
/// The header is written once at [`WavWriter::create`], declaring zero
/// frames, and re-patched in place at every [`WavWriter::patch_header`] call.
/// Samples are appended between patches; nothing is ever inserted or moved.
pub struct WavWriter {
    file: File,
    /// Frames whose sample bytes have been `write_all`'d to the file. May be
    /// ahead of `header_frames` by up to one checkpoint's worth — those bytes
    /// exist on disk but the header does not admit to them yet.
    appended_frames: u64,
    /// Frames the on-disk header currently declares. Only ever grows.
    header_frames: u64,
}

impl WavWriter {
    /// Create a new WAV file at `path` and write its initial zero-frame
    /// header. Fails if `path` already exists — a recording never resumes
    /// into an existing file.
    pub fn create(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)?;
        file.write_all(&header_bytes(0))?;
        file.sync_all()?;
        Ok(Self {
            file,
            appended_frames: 0,
            header_frames: 0,
        })
    }

    /// Append PCM samples to the data chunk. Does not fsync and does not
    /// touch the header — call [`WavWriter::fsync_data`] and
    /// [`WavWriter::patch_header`] at the next checkpoint.
    pub fn append(&mut self, samples: &[i16]) -> io::Result<()> {
        self.file.seek(SeekFrom::End(0))?;
        // WAV is little-endian regardless of host order (SPEC §3.2: s16le).
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        self.appended_frames += samples.len() as u64;
        Ok(())
    }

    /// Step 1 of the checkpoint order: durably commit appended sample bytes
    /// before anything downstream (`segments.json`) is allowed to claim them.
    pub fn fsync_data(&mut self) -> io::Result<()> {
        self.file.sync_data()
    }

    /// Step 3 of the checkpoint order: patch the header to declare
    /// `appended_frames`. RIFF size is written before `data` size, so a crash
    /// mid-patch leaves `data`'s declared length at its previous, smaller,
    /// already-fsynced value rather than a new one the bytes don't fully
    /// back yet.
    pub fn patch_header(&mut self) -> io::Result<()> {
        let frames = self.appended_frames;
        let data_len = frames * BYTES_PER_SAMPLE as u64;

        self.file.seek(SeekFrom::Start(RIFF_SIZE_OFFSET))?;
        self.file.write_all(&(36 + data_len as u32).to_le_bytes())?;

        self.file.seek(SeekFrom::Start(DATA_SIZE_OFFSET))?;
        self.file.write_all(&(data_len as u32).to_le_bytes())?;

        self.file.sync_all()?;
        self.header_frames = frames;
        Ok(())
    }

    /// Insert `frames` zero samples immediately after the header, ahead of
    /// whatever has already been appended — the head-pad contract §6 requires
    /// so frame 0 of both channels lands on the recording's shared
    /// `start_host_ns`, for whichever channel's hardware came up later.
    ///
    /// Only ever called once, at the very start of a recording, before more
    /// than a checkpoint's worth of real audio exists — so shifting the
    /// existing bytes by re-reading and rewriting them is cheap. The caller
    /// (`AudioSource::pad_leading_silence`) is responsible for serializing
    /// this against concurrent [`WavWriter::append`] calls; nothing here
    /// does that on its own.
    pub fn prepend_silence(&mut self, frames: u64) -> io::Result<()> {
        if frames == 0 {
            return Ok(());
        }
        self.file.seek(SeekFrom::Start(HEADER_LEN))?;
        let mut existing = Vec::new();
        self.file.read_to_end(&mut existing)?;

        self.file.seek(SeekFrom::Start(HEADER_LEN))?;
        let silence = vec![0u8; (frames * BYTES_PER_SAMPLE as u64) as usize];
        self.file.write_all(&silence)?;
        self.file.write_all(&existing)?;

        self.appended_frames += frames;
        Ok(())
    }

    /// Frames appended so far, including any not yet covered by the header
    /// (the excess §7 bounds at one checkpoint).
    pub fn appended_frames(&self) -> u64 {
        self.appended_frames
    }

    /// Frames the on-disk header currently declares.
    pub fn header_frames(&self) -> u64 {
        self.header_frames
    }
}

/// Read back only the frame count a WAV header at `path` declares, without
/// reading the sample data. For a caller (`meet-rec`, `drift-check`) that
/// wants to report what a *conforming* reader would see — the declared
/// length, never the file's actual on-disk size, which can run ahead of it
/// by up to one checkpoint (§7).
pub fn read_header_frames(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut header = [0u8; HEADER_LEN as usize];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" || &header[36..40] != b"data" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: not a canonical 44-byte RIFF/WAVE file", path.display()),
        ));
    }
    let declared_bytes = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;
    Ok(declared_bytes / BYTES_PER_SAMPLE as u64)
}

fn header_bytes(frames: u64) -> [u8; HEADER_LEN as usize] {
    let data_len = (frames * BYTES_PER_SAMPLE as u64) as u32;
    let byte_rate = SAMPLE_RATE_HZ * CHANNELS as u32 * BYTES_PER_SAMPLE;
    let block_align = CHANNELS * BYTES_PER_SAMPLE as u16;

    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&CHANNELS.to_le_bytes());
    h[24..28].copy_from_slice(&SAMPLE_RATE_HZ.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// Read back the frames a WAV header at `path` declares, and the samples
/// themselves. Used by tests to prove the file is genuinely playable rather
/// than merely present — a conforming reader trusts only the header's
/// declared length, never the file's actual size.
#[cfg(test)]
fn read_declared(path: &Path) -> io::Result<(u64, Vec<i16>)> {
    let mut file = File::open(path)?;
    let mut header = [0u8; HEADER_LEN as usize];
    file.read_exact(&mut header)?;
    assert_eq!(&header[0..4], b"RIFF", "not a RIFF file");
    assert_eq!(&header[8..12], b"WAVE", "not a WAVE file");
    assert_eq!(&header[36..40], b"data", "data is not the final chunk");
    let declared_bytes = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;
    let declared_frames = declared_bytes / BYTES_PER_SAMPLE as u64;

    let mut buf = vec![0u8; declared_bytes as usize];
    file.read_exact(&mut buf)?;
    let samples = buf
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect();
    Ok((declared_frames, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, freq_hz: f64) -> Vec<i16> {
        (0..frames)
            .map(|n| {
                let t = n as f64 / SAMPLE_RATE_HZ as f64;
                (i16::MAX as f64 * 0.5 * (2.0 * std::f64::consts::PI * freq_hz * t).sin()) as i16
            })
            .collect()
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("meet-ai-wav-writer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn a_header_only_file_declares_zero_frames_and_is_still_valid_wave() {
        let path = temp_path("empty.wav");
        let _ = std::fs::remove_file(&path);
        WavWriter::create(&path).unwrap();

        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(frames, 0);
        assert!(samples.is_empty());
    }

    #[test]
    fn a_graceful_stop_writes_back_exactly_what_was_appended_real_signal_not_silence() {
        let path = temp_path("graceful.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();

        let samples = tone(1600, 440.0); // 100 ms of a real 440 Hz tone
        w.append(&samples).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        assert_eq!(w.appended_frames(), 1600);
        assert_eq!(w.header_frames(), 1600);

        let (frames, read_back) = read_declared(&path).unwrap();
        assert_eq!(frames, 1600);
        assert_eq!(read_back, samples, "bytes must round-trip exactly");

        // It is not silence: a real signal has real energy.
        let rms = (read_back.iter().map(|s| (*s as f64).powi(2)).sum::<f64>()
            / read_back.len() as f64)
            .sqrt();
        assert!(rms > 1000.0, "RMS {rms} reads as silence, not a tone");
    }

    #[test]
    fn multiple_checkpoints_accumulate_correctly() {
        let path = temp_path("checkpoints.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();

        for _ in 0..5 {
            let chunk = tone(800, 440.0); // 50 ms per checkpoint
            w.append(&chunk).unwrap();
            w.fsync_data().unwrap();
            w.patch_header().unwrap();
        }

        assert_eq!(w.header_frames(), 4000);
        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(frames, 4000);
        assert_eq!(samples.len(), 4000);
    }

    /// The gate condition itself: "whatever was recorded up to that moment is
    /// playable, with a valid WAV header" after a `kill -9`. Simulated here by
    /// appending audio *past* the last header patch and never patching again
    /// — exactly what a killed process leaves behind, since the patch is the
    /// last of the three checkpoint steps.
    #[test]
    fn kill_dash_9_between_checkpoints_leaves_a_valid_playable_prefix() {
        let path = temp_path("killed.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();

        let checkpoint_1 = tone(80_000, 440.0); // 5 s, a full checkpoint
        w.append(&checkpoint_1).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        // The next checkpoint's audio lands on disk, but the process dies
        // before fsync_data/patch_header for it ever run.
        let unpatched = tone(80_000, 440.0);
        w.append(&unpatched).unwrap();
        // No fsync_data(). No patch_header(). This *is* the kill -9.
        drop(w);

        // A conforming reader only trusts the header's declared length.
        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(
            frames, 80_000,
            "header must still declare the last successful checkpoint, not the killed one"
        );
        assert_eq!(samples.len(), 80_000);
        assert_eq!(
            samples, checkpoint_1,
            "the playable prefix must be exact, not truncated audio"
        );

        // The invariant from §7: total appended bytes always exceed or equal
        // what the header admits to, bounded to one checkpoint (80_000
        // frames at 16 kHz / 5 s).
        let actual_len = std::fs::metadata(&path).unwrap().len();
        let actual_frames = (actual_len - HEADER_LEN) / BYTES_PER_SAMPLE as u64;
        assert!(actual_frames >= frames);
        assert!(
            actual_frames - frames <= 80_000,
            "excess must be bounded to one checkpoint, got {} frames",
            actual_frames - frames
        );
    }

    /// A torn header patch — process dies between the RIFF-size write and the
    /// data-size write — must not corrupt the file into an unplayable state.
    /// Since `data` size is written second, a torn patch leaves it at its
    /// previous (smaller, already-durable) value.
    #[test]
    fn a_torn_header_patch_leaves_the_data_length_at_its_old_smaller_value() {
        let path = temp_path("torn.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();

        let first = tone(1600, 440.0);
        w.append(&first).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        let second = tone(1600, 440.0);
        w.append(&second).unwrap();
        w.fsync_data().unwrap();

        // Simulate a torn patch: only the RIFF total-size field lands before
        // the crash, not the `data` chunk size.
        w.file.seek(SeekFrom::Start(RIFF_SIZE_OFFSET)).unwrap();
        let new_total = 36 + 2 * (1600 + 1600) as u32;
        w.file.write_all(&new_total.to_le_bytes()).unwrap();
        w.file.sync_all().unwrap();
        drop(w);

        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(
            frames, 1600,
            "data length must still read as the old, fully-durable value"
        );
        assert_eq!(samples, first);
    }

    #[test]
    fn prepend_silence_shifts_real_audio_after_a_silent_head_pad() {
        let path = temp_path("head-pad.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();

        // The channel that came up first already wrote its first buffer
        // before the orchestrator could measure the gap and pad the other
        // channel — exercise that ordering, not the empty-file case.
        let real = tone(160, 440.0); // 10 ms
        w.append(&real).unwrap();

        w.prepend_silence(80).unwrap(); // 5 ms pad
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        assert_eq!(w.header_frames(), 240);
        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(frames, 240);
        assert!(
            samples[..80].iter().all(|&s| s == 0),
            "the pad must be silence, not garbage"
        );
        assert_eq!(
            samples[80..],
            real,
            "the real audio must be exact and unshifted past the pad"
        );
    }

    #[test]
    fn prepend_silence_is_a_no_op_for_a_zero_frame_pad() {
        let path = temp_path("no-pad.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();
        let real = tone(160, 440.0);
        w.append(&real).unwrap();

        w.prepend_silence(0).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(frames, 160);
        assert_eq!(samples, real);
    }

    #[test]
    fn the_header_is_always_exactly_44_bytes_with_no_extra_chunks() {
        let path = temp_path("shape.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();
        w.append(&tone(160, 440.0)).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();
        drop(w);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            &bytes[36..40],
            b"data",
            "data must be the final chunk header"
        );
        assert_eq!(
            bytes.len() as u64,
            HEADER_LEN + 160 * BYTES_PER_SAMPLE as u64,
            "no chunk may sit between the header and the samples"
        );
    }
}
