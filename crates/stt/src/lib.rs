//! Speech-to-text for meet-ai.
//!
//! Phase 1 (SPEC §5). One trait, [`SttEngine`], with two implementations
//! behind it:
//!
//! * [`apple::AppleEngine`] — Apple's `SpeechTranscriber` via the
//!   `sidecar/meet-stt` Swift CLI. Default on macOS 26+, ships with the OS, no
//!   download.
//! * [`whisper::WhisperEngine`] — `whisper-rs`, the portable fallback, with
//!   [`vad`] gating in front of it.
//!
//! [`registry`] is the only module that knows which is which; L4 says the
//! engine switch must be a config change, so nothing else names an engine.
//!
//! The two things that must not regress:
//!
//! * **[`format_transcript_line`] and the §3.4 line contract.** L7 makes
//!   `transcript.md` the source of truth, so changing the format later means
//!   migrating every recorded meeting.
//! * **The silence gate.** 30 seconds of quiet must produce zero lines on both
//!   engines. See [`vad`] for how, and `tests/silence.rs` for the proof.
//!
//! Nothing in this crate reaches the network, full stop — it has no HTTP
//! client in its dependency graph. [`model`] is the model *catalogue*;
//! downloading one lives in the `modelfetch` crate, and installing an Apple
//! locale ([`apple::AppleEngine::install_locale`]) shells out to the sidecar.
//! Both are separate, explicit calls that transcription never makes.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::path::Path;

pub mod apple;
pub mod model;
pub mod registry;
pub mod replay;
pub mod segments;
pub mod session;
pub mod sink;
pub mod transcribe;
pub mod vad;

#[cfg(target_os = "macos")]
pub mod whisper;

pub use session::{
    LiveEmitter, LiveLine, LiveListener, LiveUpdate, NoListener, ReadySpan, SeqCounter,
    SessionOptions, SessionOutcome, SharedSink, SpanAssembler, SttSession,
};
pub use sink::{CollectingSink, MarkdownSink, TranscriptSink};
pub use transcribe::{MeetingPaths, Outcome, transcribe_meeting, transcribe_track};

/// Which captured track an utterance came from, and who said it (L5).
///
/// The same types `audio` records with, from `meeting-format`. Not imported
/// from `audio` itself: SPEC §8.2 forbids `crates/stt` from depending on the
/// platform-specific capture crate — that dependency is exactly how mac
/// assumptions leak into portable code and turn a Windows port into a rewrite.
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::{Channel, Speaker};

/// The §3.4 whitespace rule, shared with `store`'s reader.
///
/// Transitional re-export; new code should import from `meeting_format`.
pub use meeting_format::transcript::collapse_whitespace;

/// One finalized thing somebody said.
///
/// SPEC §2.5: only *finalized* text reaches disk. Volatile partial results go
/// to the UI event channel and are never persisted, so they never become an
/// `Utterance`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Utterance {
    /// Seconds from the start of the recording to the start of this utterance.
    ///
    /// Derived from `segments.json` (`start_host_ns + frame / rate`), never
    /// from the wall clock at write time — that is the clock-drift mitigation.
    pub start_sec: u64,
    /// `You` or `Others`, per L5.
    pub speaker: Speaker,
    /// What was said. Already whitespace-collapsed and known non-empty.
    pub text: String,
}

/// Render one `transcript.md` line: `[HH:MM:SS] Speaker: text`.
///
/// Must satisfy `^\[(\d{2}:\d{2}:\d{2})\] (You|Others): (.*)$`. The rendering
/// is `meeting_format::transcript::render_line`, the same one `store`'s reader
/// is held to; the text is not re-collapsed, because an [`Utterance`] is
/// collapsed by construction.
pub fn format_transcript_line(utterance: &Utterance) -> String {
    meeting_format::transcript::render_line(utterance.start_sec, utterance.speaker, &utterance.text)
}

/// Read a 16 kHz mono WAV into PCM samples.
///
/// Every engine in SPEC §2.5 takes this format and `meet-rec` writes it, so a
/// mismatch is a bug upstream rather than something to paper over with a
/// resampler here — `crates/audio` owns resampling (SPEC §2.3).
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<i16>, Error> {
    let mut samples = Vec::new();
    stream_wav_16k_mono(path, |block| samples.extend_from_slice(block))?;
    Ok(samples)
}

/// How much [`stream_wav_16k_mono`] hands over at a time: one second.
const WAV_BLOCK_SAMPLES: usize = vad::SAMPLE_RATE as usize;

/// Read a 16 kHz mono WAV one block at a time, never holding all of it.
///
/// Same format rules as [`read_wav_16k_mono`], which is built on this. It
/// exists for callers that only need to *look at* every sample once, such as
/// the Apple engine's silence gate. A 90-minute track is ~173 MB as `i16`,
/// and holding that just to score it frame by frame would be waste.
pub fn stream_wav_16k_mono(path: &Path, mut on_block: impl FnMut(&[i16])) -> Result<(), Error> {
    let reader =
        hound::WavReader::open(path).map_err(|e| Error::Wav(format!("{}: {e}", path.display())))?;
    let spec = reader.spec();

    if spec.channels != 1 || spec.sample_rate != vad::SAMPLE_RATE {
        return Err(Error::Wav(format!(
            "{}: expected 16 kHz mono, found {} Hz with {} channel(s)",
            path.display(),
            spec.sample_rate,
            spec.channels
        )));
    }

    let mut block = Vec::with_capacity(WAV_BLOCK_SAMPLES);
    let mut deliver = |samples: &mut dyn Iterator<Item = Result<i16, hound::Error>>| {
        for sample in samples {
            block.push(sample.map_err(|e| Error::Wav(format!("{}: {e}", path.display())))?);
            if block.len() == WAV_BLOCK_SAMPLES {
                on_block(&block);
                block.clear();
            }
        }
        if !block.is_empty() {
            on_block(&block);
        }
        Ok(())
    };

    match spec.sample_format {
        hound::SampleFormat::Int if spec.bits_per_sample == 16 => {
            deliver(&mut reader.into_samples::<i16>())
        }
        hound::SampleFormat::Float => {
            deliver(&mut reader.into_samples::<f32>().map(|sample| {
                sample.map(|value| (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            }))
        }
        _ => Err(Error::Wav(format!(
            "{}: unsupported sample format ({} bits)",
            path.display(),
            spec.bits_per_sample
        ))),
    }
}

/// A transcription backend.
///
/// Callers never construct one directly — they ask [`registry::select`] for
/// whatever `config.jsonc` chose. That is what makes "engine switch is a
/// config change only" (the Phase 1 exit gate) true by construction.
///
/// Engines emit structured utterances into a [`TranscriptSink`]; they never
/// format and never write files (SPEC §2.5).
pub trait SttEngine {
    /// Engine name, for logs and the UI's about screen.
    fn name(&self) -> &'static str;

    /// Transcribe a 16 kHz mono WAV, emitting finalized utterances only.
    ///
    /// `speaker` is fixed for the whole file: one track is one speaker (L5).
    fn transcribe(
        &mut self,
        wav: &Path,
        speaker: Speaker,
        sink: &mut dyn TranscriptSink,
    ) -> Result<(), Error>;

    /// Can this engine transcribe audio that is still arriving?
    ///
    /// SPEC §2.5 names this on the trait. An engine that answers `false` must
    /// make [`Self::start_session`] fail, and the caller falls back to
    /// transcribing the recording once it is finished.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Open a live session over audio that has not been recorded yet.
    ///
    /// Returns a trait object, so [`registry`] stays the only module that
    /// names an engine and the live pane stays a config change away from
    /// either one. See [`session`] for the volatile/finalized contract.
    ///
    /// The default is the honest answer for an engine with no streaming path:
    /// a typed error naming itself, not a silent batch fallback.
    fn start_session(
        &mut self,
        options: session::SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn session::LiveListener>,
    ) -> Result<Box<dyn session::SttSession>, Error> {
        let _ = (options, sink, listener);
        Err(Error::StreamingUnsupported(self.name()))
    }
}

/// Everything that can go wrong during transcription.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No usable engine, with a sentence explaining which ones were tried.
    #[error("{0}")]
    EngineUnavailable(String),

    /// The engine exists but failed while running.
    #[error("the speech engine failed: {0}")]
    Engine(String),

    /// The engine works, but not on audio that is still arriving.
    ///
    /// Not a fallback: the caller decides whether to record without a live
    /// pane or to pick a different engine, and both need to know which it is.
    #[error("the {0} engine cannot transcribe live audio")]
    StreamingUnsupported(&'static str),

    /// The `sidecar/meet-stt` process failed or emitted something unparseable.
    #[error("the speech helper failed: {0}")]
    Sidecar(String),

    /// The whisper model file is not where it was expected.
    #[error("no speech model at {0}")]
    ModelMissing(std::path::PathBuf),

    /// `segments.json` was missing or malformed.
    #[error("could not read segments.json: {0}")]
    Segments(String),

    /// The WAV was missing, malformed, or in the wrong format.
    #[error("could not read audio: {0}")]
    Wav(String),

    /// Reading input or writing the transcript failed.
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

    #[test]
    fn every_line_matches_the_parser_regex() {
        // SPEC §3.4 publishes this regex as the contract for anything reading
        // transcript.md. Speech containing `]` and `:` must not break it.
        let line = format_transcript_line(&Utterance {
            start_sec: 42,
            speaker: Speaker::You,
            text: "The array [0] is empty: check the index.".into(),
        });

        let (timestamp, rest) = line
            .strip_prefix('[')
            .and_then(|line| line.split_once("] "))
            .expect("prefix is anchored and fixed-width");
        let (speaker, text) = rest.split_once(": ").expect("speaker label then colon");

        assert_eq!(timestamp, "00:00:42");
        assert_eq!(speaker, "You");
        assert_eq!(text, "The array [0] is empty: check the index.");
    }

    #[test]
    fn channels_map_to_their_spec_filenames_and_speakers() {
        assert_eq!(Channel::Mic.wav_filename(), "mic.wav");
        assert_eq!(Channel::System.wav_filename(), "system.wav");
        assert_eq!(Channel::Mic.speaker(), Speaker::You);
        assert_eq!(Channel::System.speaker(), Speaker::Others);
        assert_eq!(Speaker::You.label(), "You");
        assert_eq!(Speaker::Others.label(), "Others");
    }
}
