//! Where things live inside the meetings root (SPEC §3.1).
//!
//! ```text
//! ~/Meetings/                      DEFAULT_ROOT_NAME, under the home folder
//!   .app/                          APP_DIR — the app's own files (L10: never
//!     models/                      MODELS_DIR    outside the root)
//!   2026-09-01-1430-standup/       one meeting folder
//!     meeting.md                   MEETING_FILE
//!     transcript.md                TRANSCRIPT_FILE
//!     notes.md                     NOTES_FILE
//!     audio/                       AUDIO_DIR
//!       mic.wav, system.wav        Channel::wav_filename
//!       segments.json              SEGMENTS_FILE
//! ```
//!
//! Names only, joined with `Path::join` — never a literal `/` or `~` — per the
//! Windows seam in SPEC §8.2. Finding the root itself (home folder, the user's
//! chosen location, the env override) is the app's job, not this module's.

use std::path::{Path, PathBuf};

use crate::Channel;

/// The meetings root's folder name under the home folder: `~/Meetings`.
///
/// Not read by anything yet: `src-tauri/src/meetings.rs` (`root()`) and
/// `crates/stt/src/model.rs` still spell `"Meetings"` out, and switch to this
/// and [`default_root`] in the next pass.
pub const DEFAULT_ROOT_NAME: &str = "Meetings";
/// The app's own folder inside the root: config, onboarding flag, models,
/// index. A dotfolder, so the meeting scan skips it.
pub const APP_DIR: &str = ".app";
/// Downloaded speech models, inside [`APP_DIR`].
///
/// Not read by anything yet: `crates/stt/src/model.rs`
/// (`default_model_dir`) switches to this and [`models_dir`] in the next pass.
pub const MODELS_DIR: &str = "models";
/// The app's log file and local crash files, inside [`APP_DIR`] (SPEC §3.1).
pub const LOGS_DIR: &str = "logs";

/// `meeting.md` — frontmatter plus the four fixed sections (§3.2).
pub const MEETING_FILE: &str = "meeting.md";
/// `transcript.md` — strict, append-only, one utterance per line (§3.4).
pub const TRANSCRIPT_FILE: &str = "transcript.md";
/// `notes.md` — the user's own notes.
pub const NOTES_FILE: &str = "notes.md";
/// The folder holding a meeting's two WAVs and `segments.json`.
pub const AUDIO_DIR: &str = "audio";
/// The clock-truth record, inside [`AUDIO_DIR`] (§3.4).
pub const SEGMENTS_FILE: &str = "segments.json";

/// `~/Meetings`, given the home folder. For the next pass, like
/// [`DEFAULT_ROOT_NAME`].
pub fn default_root(home: &Path) -> PathBuf {
    home.join(DEFAULT_ROOT_NAME)
}

/// `<root>/.app`.
pub fn app_dir(root: &Path) -> PathBuf {
    root.join(APP_DIR)
}

/// `<root>/.app/models`. For the next pass, like [`MODELS_DIR`].
pub fn models_dir(root: &Path) -> PathBuf {
    app_dir(root).join(MODELS_DIR)
}

/// `<root>/.app/logs`: `meet-ai.log`, its one rotated copy, and crash files.
pub fn logs_dir(root: &Path) -> PathBuf {
    app_dir(root).join(LOGS_DIR)
}

/// `<meeting>/audio`.
pub fn audio_dir(meeting: &Path) -> PathBuf {
    meeting.join(AUDIO_DIR)
}

/// `<meeting>/audio/mic.wav` or `system.wav`.
pub fn wav_path(meeting: &Path, channel: Channel) -> PathBuf {
    audio_dir(meeting).join(channel.wav_filename())
}

/// `<meeting>/audio/segments.json`.
pub fn segments_path(meeting: &Path) -> PathBuf {
    audio_dir(meeting).join(SEGMENTS_FILE)
}

/// `<meeting>/transcript.md`.
pub fn transcript_path(meeting: &Path) -> PathBuf {
    meeting.join(TRANSCRIPT_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builders_spell_out_spec_3_1() {
        let root = Path::new("home").join("Meetings");
        assert_eq!(default_root(Path::new("home")), root);
        assert_eq!(models_dir(&root), root.join(".app").join("models"));
        assert_eq!(logs_dir(&root), root.join(".app").join("logs"));

        let meeting = root.join("2026-09-01-1430-standup");
        assert_eq!(
            segments_path(&meeting),
            meeting.join("audio").join("segments.json")
        );
        assert_eq!(
            wav_path(&meeting, Channel::System),
            meeting.join("audio").join("system.wav")
        );
        assert_eq!(transcript_path(&meeting), meeting.join("transcript.md"));
    }
}
