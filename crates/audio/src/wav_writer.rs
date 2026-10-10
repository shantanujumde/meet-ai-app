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
    /// Frames already in the file when this writer opened it: 0 for one it
    /// created, the earlier segments' total for one it reopened. Where
    /// [`WavWriter::pad_segment_head`] inserts (TUR-151).
    segment_base: u64,
    /// Whether this writer created the file, so the front of the file is its
    /// own ([`WavWriter::prepend_silence`]).
    created: bool,
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
            segment_base: 0,
            created: true,
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
    /// Only for a file this writer created ([`WavWriter::create`]): on one
    /// reopened with [`WavWriter::open_append`] the front of the file is an
    /// earlier segment's audio, which a pad there would shift (TUR-151).
    /// That is an `InvalidInput` error; a reopened segment pads its own head
    /// with [`WavWriter::pad_segment_head`]. The caller
    /// (`AudioSource::pad_leading_silence`) is responsible for serializing
    /// this against concurrent [`WavWriter::append`] calls; nothing here
    /// does that on its own.
    pub fn prepend_silence(&mut self, frames: u64) -> io::Result<()> {
        if !self.created {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "prepend_silence on a reopened WAV would shift the earlier segments; \
                 pad the segment's own head instead",
            ));
        }
        self.insert_silence(0, frames)
    }

    /// Contract §6's head-pad for the segment this writer is writing: insert
    /// `frames` zero samples where this writer started appending (frame 0 of
    /// a file it created, the end of the earlier segments of one it
    /// reopened), ahead of the samples it has appended since (TUR-151).
    ///
    /// Only those samples move, never an earlier segment's, so the bytes
    /// rewritten are this segment's first moments, not the whole file, and a
    /// crash mid-rewrite cannot touch audio from before a device switch. Same
    /// serialization rule as [`WavWriter::prepend_silence`].
    pub fn pad_segment_head(&mut self, frames: u64) -> io::Result<()> {
        self.insert_silence(self.segment_base, frames)
    }

    /// Insert `frames` zero samples at frame `at`, shifting the samples
    /// appended after it.
    fn insert_silence(&mut self, at: u64, frames: u64) -> io::Result<()> {
        if frames == 0 {
            return Ok(());
        }
        let offset = HEADER_LEN + at * BYTES_PER_FRAME;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut existing = Vec::new();
        self.file.read_to_end(&mut existing)?;

        self.file.seek(SeekFrom::Start(offset))?;
        let silence = vec![0u8; (frames * BYTES_PER_FRAME) as usize];
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
            segment_base: existing_frames,
            created: false,
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
mod tests;
