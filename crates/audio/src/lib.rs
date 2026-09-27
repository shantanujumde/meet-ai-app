//! Audio capture for meet-ai.
//!
//! Phase 0 territory (SPEC §5). Nothing here is implemented yet — this crate
//! exists so the capture work has a place to land that already compiles, is
//! already wired into `just check`, and already has the platform seam in the
//! right place.
//!
//! The one thing that is real today is [`AudioSource`]. SPEC §4 requires the
//! trait to exist from day one even though only macOS will implement it, so
//! that the Windows port (SPEC §8.2) is additive rather than a rewrite.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;

/// The macOS capture implementation. SPEC §4 ⛔: OS-specific code lives here
/// and nowhere else.
#[cfg(target_os = "macos")]
pub mod macos;

/// The recording-start chime and its detector (SPEC §8.1, decided in A6).
/// Platform-agnostic for the same reason `segments` is: it is the contract
/// between the side that plays the chime and the side that looks for it, and
/// both ends have to agree on it exactly.
pub mod chime;

/// The `segments.json` contract and the drift maths that reads it (SPEC §3.4,
/// amended by A5). Platform-agnostic on purpose: it is a file format, and
/// `drift-check` has to parse it wherever a recording is read.
pub mod segments;

/// Which side of the conversation a stream came from.
///
/// L5 locks speaker labelling to the two channels we capture: the microphone is
/// the person using this Mac, the process tap is everyone else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// The local microphone. Rendered as `You` in `transcript.md`.
    Mic,
    /// The system audio process tap. Rendered as `Others`.
    System,
}

impl Channel {
    /// The file this channel is recorded to, relative to the meeting folder.
    pub fn wav_filename(self) -> &'static str {
        match self {
            Channel::Mic => "mic.wav",
            Channel::System => "system.wav",
        }
    }

    /// The speaker label this channel produces in `transcript.md` (SPEC §3.4).
    pub fn speaker_label(self) -> &'static str {
        match self {
            Channel::Mic => "You",
            Channel::System => "Others",
        }
    }
}

/// A platform's implementation of one capture channel.
///
/// Implementations are expected to own their own OS threads. Audio callbacks
/// must never run on the tokio runtime (SPEC §2.3).
pub trait AudioSource {
    /// Start writing 16 kHz mono PCM to `dest`, returning once capture is live.
    fn start(&mut self, dest: PathBuf) -> Result<(), Error>;

    /// Stop capture and flush the WAV header.
    fn stop(&mut self) -> Result<(), Error>;

    /// Which channel this source feeds.
    fn channel(&self) -> Channel;
}

/// Everything that can go wrong during capture.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The user has not granted microphone or audio-capture permission.
    ///
    /// The UI turns this into the permission-denied onboarding path, so it is a
    /// distinct variant rather than a string.
    #[error("meet-ai does not have permission to record audio")]
    PermissionDenied,

    /// No usable input or output device was found.
    #[error("no audio device available: {0}")]
    NoDevice(String),

    /// Writing the WAV file failed.
    #[error("audio i/o failed")]
    Io(#[from] std::io::Error),

    /// Capture is not implemented for this platform yet.
    #[error("audio capture is not supported on this platform")]
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_map_to_their_spec_filenames_and_labels() {
        assert_eq!(Channel::Mic.wav_filename(), "mic.wav");
        assert_eq!(Channel::System.wav_filename(), "system.wav");
        assert_eq!(Channel::Mic.speaker_label(), "You");
        assert_eq!(Channel::System.speaker_label(), "Others");
    }
}
