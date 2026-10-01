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

// The canonical header's shape. Public because it is a promise, not a detail:
// §13 lets a live reader tail at `HEADER_LEN + BYTES_PER_FRAME * frame`, and
// the app's cut-short-recording repair (`src-tauri/src/meetings.rs`) compares a
// file's real length against these and patches the two size fields in place.
// One definition, so a reader cannot restate a number this writer changed.

/// Mono: one sample per frame.
pub const CHANNELS: u16 = 1;
/// 16-bit signed PCM.
pub const BITS_PER_SAMPLE: u16 = 16;
/// Bytes in one sample.
pub const BYTES_PER_SAMPLE: u32 = (BITS_PER_SAMPLE / 8) as u32;
/// Bytes in one frame — one sample per channel. `2` for mono 16-bit.
pub const BYTES_PER_FRAME: u64 = BYTES_PER_SAMPLE as u64 * CHANNELS as u64;
/// The canonical header is exactly this long, and samples start right after.
pub const HEADER_LEN: u64 = 44;

/// Offset of the RIFF chunk's size field (bytes 4..8): `36 + data_len`.
pub const RIFF_SIZE_OFFSET: u64 = 4;
/// Offset of the `data` chunk's size field (bytes 40..44): `frames * 2`.
pub const DATA_SIZE_OFFSET: u64 = 40;

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
    /// `appended_frames` as of the most recent [`WavWriter::fsync_data`] call —
    /// the count [`WavWriter::patch_header`] is allowed to declare. A worker
    /// thread may keep appending (unsynced) frames between a caller's
    /// `fsync_data` and `patch_header` calls (e.g. while the caller writes
    /// `segments.json` in between, per contract §7); patching against the
    /// live `appended_frames` at that later moment would let the header
    /// declare more frames than `segments.json` was told about, breaking the
    /// `sum(*_frames) >= header_frames` invariant in the one direction that
    /// isn't safe. Freezing the count at `fsync_data` time keeps the header
    /// always at or behind whatever `position()` reported afterward.
    synced_frames: u64,
    /// Frames the on-disk header currently declares. Only ever grows.
    header_frames: u64,
    /// Little-endian bytes of the chunk being appended, reused across calls so
    /// the per-chunk path allocates nothing once it has grown to chunk size.
    byte_scratch: Vec<u8>,
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
            synced_frames: 0,
            header_frames: 0,
            byte_scratch: Vec::new(),
        })
    }

    /// Append PCM samples to the data chunk. Does not fsync and does not
    /// touch the header — call [`WavWriter::fsync_data`] and
    /// [`WavWriter::patch_header`] at the next checkpoint.
    pub fn append(&mut self, samples: &[i16]) -> io::Result<()> {
        self.file.seek(SeekFrom::End(0))?;
        // WAV is little-endian regardless of host order (SPEC §3.2: s16le).
        self.byte_scratch.clear();
        for sample in samples {
            self.byte_scratch.extend_from_slice(&sample.to_le_bytes());
        }
        self.file.write_all(&self.byte_scratch)?;
        self.appended_frames += samples.len() as u64;
        Ok(())
    }

    /// Step 1 of the checkpoint order: durably commit appended sample bytes
    /// before anything downstream (`segments.json`) is allowed to claim them.
    /// Freezes `synced_frames` at the current `appended_frames` so a later
    /// [`WavWriter::patch_header`] call can't declare frames appended (by a
    /// concurrent worker thread) after this sync, but before the header is
    /// actually patched.
    pub fn fsync_data(&mut self) -> io::Result<()> {
        self.file.sync_data()?;
        self.synced_frames = self.appended_frames;
        Ok(())
    }

    /// Step 3 of the checkpoint order: patch the header to declare
    /// `synced_frames` — the count as of the last [`WavWriter::fsync_data`]
    /// call, not whatever `appended_frames` has grown to since. RIFF size is
    /// written before `data` size, so a crash mid-patch leaves `data`'s
    /// declared length at its previous, smaller, already-fsynced value rather
    /// than a new one the bytes don't fully back yet.
    pub fn patch_header(&mut self) -> io::Result<()> {
        let frames = self.synced_frames;
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

    /// Reopen an existing WAV file — one written by an earlier
    /// [`WavWriter`] for an earlier segment of the *same* recording — and
    /// continue appending after its declared audio, instead of truncating.
    ///
    /// Contract: "one WAV per channel, segments concatenated in `idx`
    /// order" (`crates/audio/src/segments.rs`). A segment boundary (a device
    /// change mid-call) tears down and rebuilds the OS-level capture stream,
    /// but the archive file itself must stay one continuous per-channel WAV
    /// — so the new segment's writer has to pick up exactly where the old
    /// one's last `patch_header` left off, not start a fresh file.
    ///
    /// Seeks to the header's *declared* length, not the file's actual size,
    /// and truncates away anything past it with `File::set_len`. A graceful
    /// segment close (`fsync_data` + `patch_header`, same as
    /// `AudioSource::stop`) leaves the two equal, so this is a no-op in the
    /// normal case; it only bites if a previous run was killed mid-checkpoint
    /// and left up to one checkpoint's worth of undeclared bytes past the
    /// header (§7). Discarding rather than keeping that excess is
    /// deliberate: those bytes were never accounted for in any
    /// `segments.json` this writer's caller controls, and keeping them would
    /// silently insert audio no anchor ever measured.
    ///
    /// `header_frames`/`synced_frames`/`appended_frames` all start at the
    /// *previous* declared count and grow from there — the header always
    /// describes the whole file, not just the reopened segment.
    pub fn open_append(path: &Path) -> io::Result<Self> {
        let existing_frames = read_header_frames(path)?;
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let data_end = HEADER_LEN + existing_frames * BYTES_PER_SAMPLE as u64;
        file.set_len(data_end)?;
        file.seek(SeekFrom::Start(data_end))?;
        Ok(Self {
            file,
            appended_frames: existing_frames,
            synced_frames: existing_frames,
            header_frames: existing_frames,
            byte_scratch: Vec::new(),
        })
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
    fn append_reuses_its_byte_buffer_in_steady_state() {
        let path = temp_path("scratch-capacity.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();
        w.append(&tone(400, 440.0)).unwrap();
        let cap = w.byte_scratch.capacity();
        for len in [341, 342, 400, 17, 399] {
            w.append(&tone(len, 440.0)).unwrap();
            assert_eq!(w.byte_scratch.capacity(), cap);
        }
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

    /// Reproduces the exact bug found on real hardware (TUR-54, force-quit at
    /// 14s / last checkpoint at 10s): the header ended up declaring 342 more
    /// frames than `segments.json` had been told about. Root cause was a
    /// concurrent worker thread appending more audio in the window between a
    /// checkpoint's `fsync_data()` (after which the caller reads `position()`
    /// and commits a frame count to `segments.json`) and that same
    /// checkpoint's `patch_header()` — which used to re-read the *live*
    /// `appended_frames`, ahead of what `segments.json` had just committed to.
    /// `sum(*_frames) >= header_frames` must hold in that direction always;
    /// this fixture is the other direction and must never reproduce.
    #[test]
    fn patch_header_never_declares_more_than_the_last_fsync_even_if_more_was_appended_since() {
        let path = temp_path("racing-append.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path).unwrap();

        let checkpoint = tone(80_000, 440.0); // 5 s, a full checkpoint
        w.append(&checkpoint).unwrap();
        w.fsync_data().unwrap();
        // `segments.json` would be written here, committing to 80_000 frames.
        // Simulate the worker thread landing one more chunk in that window,
        // before this checkpoint's patch_header() runs.
        w.append(&tone(1_600, 440.0)).unwrap(); // 100ms, unsynced

        w.patch_header().unwrap();

        assert_eq!(
            w.header_frames(),
            80_000,
            "header must declare exactly what was fsynced and committed to segments.json, \
             not the extra frames a concurrent append landed afterward"
        );
        let (frames, _) = read_declared(&path).unwrap();
        assert_eq!(frames, 80_000);
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

    /// A device-change segment reopen: the old segment's writer closes
    /// cleanly, a *new* `WavWriter` reopens the same path, and its own
    /// checkpoints must land after the first segment's audio, not overwrite
    /// or duplicate it — this is what makes "one WAV per channel, segments
    /// concatenated in `idx` order" true on disk, not just in `segments.json`.
    #[test]
    fn open_append_continues_the_same_file_across_a_segment_reopen() {
        let path = temp_path("reopen.wav");
        let _ = std::fs::remove_file(&path);

        let first_segment = tone(1600, 440.0); // 100ms
        {
            let mut w = WavWriter::create(&path).unwrap();
            w.append(&first_segment).unwrap();
            w.fsync_data().unwrap();
            w.patch_header().unwrap();
            assert_eq!(w.header_frames(), 1600);
        }

        let second_segment = tone(800, 880.0); // 50ms, a different tone
        let mut w = WavWriter::open_append(&path).unwrap();
        assert_eq!(
            w.header_frames(),
            1600,
            "reopening must start from the prior segment's declared count, not zero"
        );
        w.append(&second_segment).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        assert_eq!(w.header_frames(), 2400);
        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(frames, 2400);
        assert_eq!(&samples[..1600], &first_segment[..], "first segment intact");
        assert_eq!(
            &samples[1600..],
            &second_segment[..],
            "second segment appended immediately after, not overlapping"
        );
    }

    /// The prior segment's writer was killed before it could patch its
    /// header (contract §7's bounded excess): `open_append` must discard the
    /// undeclared tail rather than keep it as an unaccounted gap in the
    /// middle of the file.
    #[test]
    fn open_append_truncates_undeclared_bytes_left_by_a_prior_kill_dash_9() {
        let path = temp_path("reopen-after-kill.wav");
        let _ = std::fs::remove_file(&path);

        {
            let mut w = WavWriter::create(&path).unwrap();
            w.append(&tone(1600, 440.0)).unwrap();
            w.fsync_data().unwrap();
            w.patch_header().unwrap(); // header now declares 1600

            // Simulate a kill -9: more audio lands on disk but is never
            // fsynced or declared.
            w.append(&tone(400, 440.0)).unwrap();
            drop(w);
        }

        let actual_len_before = std::fs::metadata(&path).unwrap().len();
        assert!(
            actual_len_before > HEADER_LEN + 1600 * BYTES_PER_SAMPLE as u64,
            "the undeclared tail must actually be on disk for this test to mean anything"
        );

        let second_segment = tone(400, 880.0);
        let mut w = WavWriter::open_append(&path).unwrap();
        assert_eq!(w.header_frames(), 1600);
        w.append(&second_segment).unwrap();
        w.fsync_data().unwrap();
        w.patch_header().unwrap();

        assert_eq!(
            w.header_frames(),
            2000,
            "must be exactly the declared 1600 plus the new 400, not the killed tail too"
        );
        let (frames, samples) = read_declared(&path).unwrap();
        assert_eq!(frames, 2000);
        assert_eq!(
            &samples[1600..],
            &second_segment[..],
            "the second segment must follow immediately, with the killed tail discarded"
        );
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
