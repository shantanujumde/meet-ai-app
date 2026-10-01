//! Telling a finished recording from a cut-short one, and repairing the
//! second kind, by looking only at the WAV headers and `segments.json`.
//!
//! Moved here from the app shell so the rule sits beside the writer that
//! produces the shapes it judges ([`crate::wav_writer`], [`crate::segments`]).

use std::fs::{self, OpenOptions};
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use crate::Channel;
use crate::segments::{Segments, SegmentsDrift as _};
// The canonical header `wav_writer` writes, and the size of one mono 16-bit
// frame: the audio check below compares a file's real length against what its
// header declares, and `read_header_frames` refuses any file that is not
// exactly this shape. Also where the RIFF and `data` size fields live.
use crate::wav_writer::{
    BYTES_PER_FRAME as WAV_BYTES_PER_FRAME, DATA_SIZE_OFFSET as WAV_DATA_SIZE_OFFSET,
    HEADER_LEN as WAV_HEADER_LEN, RIFF_SIZE_OFFSET as WAV_RIFF_SIZE_OFFSET, read_header_frames,
};

const SEGMENTS: &str = meeting_format::layout::SEGMENTS_FILE;

/// What a meeting's `audio/` folder says about how its recording ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioCheck {
    pub ended_cleanly: bool,
    /// The longer track's header-declared length, in frames. `None` when
    /// there is no WAV to read.
    pub header_frames: Option<u64>,
}

/// Did this recording stop on purpose?
///
/// SPEC A5 §3 (contract §7): `sum(*_frames) >= wav_header_frames` always,
/// **with equality after a graceful stop** — and `RecordingSession::stop`
/// finalises both headers to every sample written before it writes the last
/// `segments.json`. So a cleanly stopped recording is exactly this, for every
/// WAV present in `audio/`:
///
/// 1. `segments.json` is there and parses. The recorder writes it on every
///    stop and every checkpoint, even when the system tap failed (SPEC §3.4),
///    so a WAV with none beside it was killed before its first checkpoint —
///    which on v0.3.0, where checkpoints never ran, is every killed recording.
/// 2. The header is canonical and declares exactly the bytes on disk. A kill
///    leaves samples appended after the last header patch: up to one
///    checkpoint's worth, or all of them on v0.3.0, whose headers stay at 0.
/// 3. The header declares exactly the frames `segments.json` gives that
///    channel. After a kill it lags them (A5's benign crash window); running
///    ahead of them is something the writer cannot produce at all.
///
/// Any one failing means interrupted. No WAVs at all means there is nothing
/// to judge by, and the meeting is treated as finished: that is a folder from
/// before recording existed, or one whose audio retention (L16) has deleted.
///
/// Cost: one small JSON read, then a 44-byte read and a `stat` per WAV. The
/// samples are never read.
///
/// **The one kill this cannot see** lands after a checkpoint's header patch
/// and before the next sample reaches the file. Header, file and segments
/// then agree exactly — `segments.json` has no stop marker, and a stop's
/// final anchor looks like a checkpoint's — so it reads as finished. The
/// window is the gap between two buffer flushes, and a meeting caught in it
/// loses only the label: everything on disk is declared and timestamped.
pub fn classify_audio(audio_dir: &Path) -> AudioCheck {
    let wavs: Vec<(Channel, PathBuf)> = [Channel::Mic, Channel::System]
        .into_iter()
        .map(|channel| (channel, audio_dir.join(channel.wav_filename())))
        .filter(|(_, path)| path.is_file())
        .collect();
    if wavs.is_empty() {
        return AudioCheck {
            ended_cleanly: true,
            header_frames: None,
        };
    }

    let segments = fs::read_to_string(audio_dir.join(SEGMENTS))
        .ok()
        .and_then(|raw| Segments::from_json(&raw).ok());
    let mut ended_cleanly = segments.is_some();
    let mut longest: Option<u64> = None;

    for (channel, path) in &wavs {
        let (Ok(header_frames), Ok(meta)) = (read_header_frames(path), fs::metadata(path)) else {
            // Not a header meet-ai wrote, or shorter than one. Either way this
            // is not a recording that was stopped and finalised.
            ended_cleanly = false;
            continue;
        };
        longest = Some(longest.map_or(header_frames, |l| l.max(header_frames)));

        let declared_len = WAV_HEADER_LEN + header_frames * WAV_BYTES_PER_FRAME;
        if meta.len() != declared_len {
            ended_cleanly = false;
        }
        if let Some(segments) = &segments
            && segments.total_frames(*channel) != header_frames
        {
            ended_cleanly = false;
        }
    }

    AudioCheck {
        ended_cleanly,
        header_frames: longest,
    }
}

/// Raise a canonical WAV header's declared length to cover every whole frame
/// on disk. `Ok(false)` when it already does, or when the file is not a
/// header this app wrote (which is then never touched).
pub fn expose_unheadered_samples(path: &Path) -> std::io::Result<bool> {
    let declared = read_header_frames(path)?;
    let on_disk = fs::metadata(path)?.len().saturating_sub(WAV_HEADER_LEN) / WAV_BYTES_PER_FRAME;
    // Both size fields are u32, and RIFF's counts the 36 header bytes after
    // it. Past ~37 hours of audio the header simply cannot say more.
    let most = (u64::from(u32::MAX) - (WAV_HEADER_LEN - 8)) / WAV_BYTES_PER_FRAME;
    let target = on_disk.min(most);
    if target <= declared {
        return Ok(false);
    }

    let data_bytes = target * WAV_BYTES_PER_FRAME;
    let riff_bytes = data_bytes + (WAV_HEADER_LEN - 8);
    let mut file = OpenOptions::new().write(true).open(path)?;
    // RIFF first, `data` last: `read_header_frames` goes by `data`, so a crash
    // between the two writes leaves the header reading as unrepaired and the
    // next launch redoes both, instead of stranding a stale RIFF size.
    file.seek(SeekFrom::Start(WAV_RIFF_SIZE_OFFSET))?;
    file.write_all(&(riff_bytes as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(WAV_DATA_SIZE_OFFSET))?;
    file.write_all(&(data_bytes as u32).to_le_bytes())?;
    file.sync_all()?;
    Ok(true)
}
