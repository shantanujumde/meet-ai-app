//! The facts about a meeting folder that more than one crate has to agree on.
//!
//! SPEC §3.1 fixes the folder layout, §3.4 fixes `transcript.md` and
//! `segments.json`, and L5 fixes the two channels and the two speakers. Each of
//! those used to be written down once per crate that touched it — `audio`
//! wrote `segments.json` against one struct and `stt` read it against another,
//! and the two had already drifted apart (the reader had no `anchors`, and
//! typed `version` differently). A contract with two definitions is a contract
//! nobody checks. This crate is the one definition; `audio`, `stt` and `store`
//! re-export what they used to own, so their old paths still work.
//!
//! * [`layout`] — file and folder names inside the meetings root (§3.1).
//! * [`segments`] — the `segments.json` schema (§3.4 + SPEC A5).
//! * [`transcript`] — the `transcript.md` line format (§3.4).
//! * [`Channel`], [`Speaker`], [`SAMPLE_RATE`] — L5 and SPEC §2.3.
//! * [`write_atomic`] — the one crash-safe way to replace a file.
//!
//! Platform-free by construction: serde is the only dependency and there is no
//! `#[cfg(target_os)]` anywhere, because `stt` and `store` must stay free of
//! mac-only code (SPEC §8.2) and they both depend on this.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::fs::File;
use std::io::{self, Write as _};
use std::path::Path;

pub mod layout;
pub mod segments;
pub mod transcript;

/// The rate of every WAV a meeting holds, and the rate every engine reads.
///
/// SPEC §2.3: `crates/audio` resamples whatever the hardware delivers down to
/// this before anything reaches disk, so a WAV at any other rate is a bug
/// upstream, never something a reader should resample around. SPEC §2.5's
/// engines and `earshot` all take it too.
pub const SAMPLE_RATE: u32 = 16_000;

/// Which side of the conversation a stream came from.
///
/// L5 locks speaker labelling to the two channels we capture: the microphone is
/// the person using this Mac, the process tap is everyone else. Getting a
/// speaker wrong is therefore a file-naming bug, not an accuracy problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// The local microphone. Rendered as `You` in `transcript.md`.
    Mic,
    /// The system audio process tap. Rendered as `Others`.
    System,
}

impl Channel {
    /// The file this channel is recorded to, relative to the meeting's
    /// [`layout::AUDIO_DIR`].
    pub fn wav_filename(self) -> &'static str {
        match self {
            Channel::Mic => "mic.wav",
            Channel::System => "system.wav",
        }
    }

    /// The speaker this channel is attributed to (L5).
    pub fn speaker(self) -> Speaker {
        match self {
            Channel::Mic => Speaker::You,
            Channel::System => Speaker::Others,
        }
    }

    /// The speaker label this channel produces in `transcript.md` (SPEC §3.4).
    pub fn speaker_label(self) -> &'static str {
        self.speaker().label()
    }
}

/// The two speaker labels v1 can produce (L5).
///
/// Serializes lowercase (`"you"`/`"others"`), which is what the live
/// transcript events carry. That is a wire name, not the file's: the label
/// written into `transcript.md` is [`Speaker::label`], capitalised, and a
/// reader that hands parsed lines to the UI serializes that instead (see
/// `store::transcript::Line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    /// The microphone channel — the person using this Mac.
    You,
    /// The system-audio channel — everyone else on the call.
    Others,
}

impl Speaker {
    /// The literal label written into `transcript.md`.
    pub fn label(self) -> &'static str {
        match self {
            Speaker::You => "You",
            Speaker::Others => "Others",
        }
    }
}

/// Replace `path` with `bytes` so a crash at any instant leaves either the old
/// file or the new one, never a torn one.
///
/// A temp file in the same folder (same filesystem, so the rename is atomic),
/// `fsync`'d, then `rename(2)`'d over `path`, then the folder itself
/// `fsync`'d. The file sync makes the *contents* durable before the rename can
/// publish them; the folder sync makes the *rename* durable — without it a
/// power cut after a successful return can still bring back the old file,
/// which for `segments.json` is a checkpoint silently lost (§7/§11).
///
/// The temp name is a dotfile (`.{name}.tmp.{pid}`): the folder scan in
/// `store` already skips dotfiles, and the pid keeps two processes writing the
/// same file from sharing one. On failure the temp file is removed, best
/// effort, so a failed save leaves no litter.
///
/// Does not create `path`'s folder; a caller that may be first to write there
/// creates it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no file name to write", path.display()),
        )
    })?;
    let tmp = dir.join(format!(
        ".{}.tmp.{}",
        name.to_string_lossy(),
        std::process::id()
    ));

    let written = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    written?;
    sync_dir(dir)
}

/// `fsync` a folder so a rename inside it survives a power cut.
///
/// Unix only: Windows has no directory handle to flush, and `MoveFileEx` is
/// already as durable as that platform offers. A filesystem that cannot sync a
/// directory at all (some network mounts answer `EINVAL`) is treated as done —
/// the rename already happened, and failing the whole write over a durability
/// hint the filesystem does not support would turn a saved file into an error.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    match File::open(dir).and_then(|dir| dir.sync_all()) {
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        other => other,
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("meeting-format-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_rewrite_replaces_the_file_and_leaves_no_temp_file() {
        let dir = scratch("rewrite");
        let path = dir.join(layout::SEGMENTS_FILE);
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(names(&dir), [layout::SEGMENTS_FILE]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_rename_cleans_up_its_temp_file() {
        // A non-empty folder where the file should be: the temp file writes
        // fine and the rename over it fails.
        let dir = scratch("failed");
        std::fs::create_dir_all(dir.join(layout::NOTES_FILE).join("child")).unwrap();
        assert!(write_atomic(&dir.join(layout::NOTES_FILE), b"draft").is_err());
        assert_eq!(names(&dir), [layout::NOTES_FILE]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_path_with_no_file_name_is_refused_not_written_somewhere_else() {
        assert!(write_atomic(Path::new("/"), b"x").is_err());
    }

    #[test]
    fn each_channel_names_its_file_and_its_speaker() {
        assert_eq!(Channel::Mic.wav_filename(), "mic.wav");
        assert_eq!(Channel::System.wav_filename(), "system.wav");
        assert_eq!(Channel::Mic.speaker(), Speaker::You);
        assert_eq!(Channel::System.speaker_label(), "Others");
    }

    #[test]
    fn the_wire_names_are_lowercase_and_the_file_labels_are_not() {
        assert_eq!(
            serde_json::to_string(&Channel::System).unwrap(),
            "\"system\""
        );
        assert_eq!(serde_json::to_string(&Speaker::You).unwrap(), "\"you\"");
        assert_eq!(Speaker::You.label(), "You");
    }
}
