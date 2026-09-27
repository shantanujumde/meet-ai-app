//! Speech-to-text for meet-ai.
//!
//! Phase 1 territory (SPEC §5). Nothing transcribes yet. What is real here is
//! the seam every engine has to fit through:
//!
//! * [`SttEngine`] — L4 locks Apple `SpeechTranscriber` (via `sidecar/meet-stt`)
//!   as the default on macOS 26+ and `whisper-rs` as the fallback. Switching
//!   between them must be a config change only, so nothing outside this crate
//!   ever names an engine.
//! * [`Utterance`] and [`format_transcript_line`] — SPEC §3.4 makes
//!   `transcript.md` the source of truth, so the line contract has to hold
//!   before the first meeting is ever recorded.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::path::Path;

/// One finalized thing somebody said.
///
/// SPEC §2.5: only *finalized* text reaches disk. Volatile partial results go to
/// the UI event channel and are never persisted, so they never become an
/// `Utterance`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Utterance {
    /// Seconds from the start of the recording to the start of this utterance.
    ///
    /// Derived from `segments.json` (`start_host_ns + frame / rate`), never from
    /// the wall clock at write time — that is the clock-drift mitigation.
    pub start_sec: u64,
    /// `You` or `Others`, per L5.
    pub speaker: Speaker,
    /// What was said. Already whitespace-collapsed and known non-empty.
    pub text: String,
}

/// The two speaker labels v1 can produce (L5).
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

/// Collapse recognized text the way SPEC §3.4 requires.
///
/// One utterance is exactly one line, so every `\n`, `\r`, `\t` and run of
/// spaces becomes a single space. Returns `None` for whitespace-only input —
/// that is the last line of defence against a hallucinated empty segment.
pub fn collapse_whitespace(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

/// Render one `transcript.md` line: `[HH:MM:SS] Speaker: text`.
///
/// Must satisfy `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`. No escaping —
/// the prefix is fixed-width and anchored, so `]` and `:` inside speech are safe.
pub fn format_transcript_line(utterance: &Utterance) -> String {
    let (h, m, s) = (
        utterance.start_sec / 3600,
        (utterance.start_sec % 3600) / 60,
        utterance.start_sec % 60,
    );
    format!(
        "[{h:02}:{m:02}:{s:02}] {}: {}",
        utterance.speaker.label(),
        utterance.text
    )
}

/// A transcription backend.
///
/// Callers never construct one directly — they ask the engine registry for
/// whatever `config.jsonc` selected. That is what makes "engine switch is a
/// config change only" (the Phase 1 exit gate) true by construction.
pub trait SttEngine {
    /// Human-readable engine name, for logs and the UI's about screen.
    fn name(&self) -> &'static str;

    /// Transcribe a 16 kHz mono WAV, emitting finalized utterances only.
    fn transcribe(&mut self, wav: &Path, speaker: Speaker) -> Result<Vec<Utterance>, Error>;
}

/// Everything that can go wrong during transcription.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The selected engine is not available on this machine.
    ///
    /// The Apple engine needs macOS 26; the whisper engine needs a downloaded
    /// model. The UI turns this into a sentence telling the user which one.
    #[error("speech engine `{0}` is not available on this Mac")]
    EngineUnavailable(&'static str),

    /// The `sidecar/meet-stt` process failed or emitted something unparseable.
    #[error("the speech helper failed: {0}")]
    Sidecar(String),

    /// Reading the WAV or writing the transcript failed.
    #[error("transcript i/o failed")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_utterance_is_exactly_one_line() {
        let collapsed = collapse_whitespace("Sessions are still\n\tin   memory,\r\nthat's it.");
        assert_eq!(
            collapsed.as_deref(),
            Some("Sessions are still in memory, that's it.")
        );
    }

    #[test]
    fn whitespace_only_text_is_never_written() {
        assert_eq!(collapse_whitespace("   \n\t  "), None);
        assert_eq!(collapse_whitespace(""), None);
    }

    #[test]
    fn transcript_line_matches_the_spec_shape() {
        let line = format_transcript_line(&Utterance {
            start_sec: 3_671,
            speaker: Speaker::Others,
            text: "Morning everyone: let's start.".into(),
        });
        assert_eq!(line, "[01:01:11] Others: Morning everyone: let's start.");
    }
}
