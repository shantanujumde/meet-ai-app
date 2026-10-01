//! The Apple `SpeechTranscriber` engine, driven through `sidecar/meet-stt`.
//!
//! SPEC §2.6: the sidecar is a Swift CLI that takes a WAV path and writes JSON
//! lines. This module is the other half of that contract. Everything it parses
//! is documented at the top of `sidecar/meet-stt/main.swift`; the two files
//! move together.
//!
//! Why a subprocess at all: `SpeechAnalyzer` is Swift-concurrency-native and
//! is not reachable through `objc2` (SPEC A2). Why that is safe: transcription
//! reads a file off disk, so there is no microphone and nothing for TCC to
//! attribute — the sidecar TCC risk applies only to *capture*.

mod live;
mod protocol;
#[cfg(test)]
mod testutil;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::session::{LiveListener, SessionOptions, SttSession};
use crate::sink::TranscriptSink;
use crate::vad::SpeechTimeline;
use crate::{Error, Speaker, Utterance, collapse_whitespace};

use live::AppleSession;
pub use protocol::Probe;
use protocol::{Line, over_heard_speech};

/// Apple's on-device speech engine (macOS 26+).
pub struct AppleEngine {
    binary: PathBuf,
    locale: String,
}

impl AppleEngine {
    pub const NAME: &'static str = crate::registry::APPLE_SPEECH;

    /// Use a specific `meet-stt` binary.
    pub fn new(binary: PathBuf, locale: impl Into<String>) -> Self {
        Self {
            binary,
            locale: locale.into(),
        }
    }

    /// Find `meet-stt` the way the shipped app will.
    ///
    /// In a bundle it sits beside the executable at `Contents/MacOS/meet-stt`.
    /// In development `just sidecar` writes it to `target/meet-stt`, so both
    /// are checked and the bundle location wins.
    pub fn discover() -> Option<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("meet-stt"));
            // `cargo test` binaries live in target/debug/deps/, so walk up to
            // the target dir `just sidecar` actually writes to.
            candidates.push(dir.join("../meet-stt"));
            candidates.push(dir.join("../../meet-stt"));
        }
        if let Ok(from_env) = std::env::var("MEET_STT_BIN") {
            candidates.insert(0, PathBuf::from(from_env));
        }

        candidates.into_iter().find(|path| path.is_file())
    }

    /// Ask the sidecar what it can do. Cheap — no model is loaded.
    pub fn probe(binary: &Path, locale: &str) -> Result<Probe, Error> {
        let output = Command::new(binary)
            .args(["--probe", "--locale", locale])
            .output()
            .map_err(|e| Error::Sidecar(format!("could not run {}: {e}", binary.display())))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<Line>(line) {
                Ok(Line::Probe(probe)) => return Ok(*probe),
                Ok(Line::Error { code, message }) => {
                    return Err(Error::Sidecar(format!("{code}: {message}")));
                }
                _ => continue,
            }
        }
        Err(Error::Sidecar(format!(
            "--probe produced no probe line (stderr: {})",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    /// Download the on-device model for a locale.
    ///
    /// This is the Apple equivalent of the whisper model download and the only
    /// part of this engine that uses the network. It is deliberately a separate
    /// entry point so that no transcription code path can ever reach it.
    pub fn install_locale(binary: &Path, locale: &str) -> Result<(), Error> {
        let status = Command::new(binary)
            .args(["--install-locale", "--locale", locale])
            .status()
            .map_err(|e| Error::Sidecar(format!("could not run {}: {e}", binary.display())))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Sidecar(format!(
                "--install-locale exited with {status}"
            )))
        }
    }
}

/// Drain `meet-stt <wav>`'s stdout into `sink`.
///
/// Every final that passes the silence gate is written. A sidecar `error` line
/// is handed back instead of returned, because the caller still has to reap
/// the process before it reports anything.
fn write_batch_finals(
    stdout: impl BufRead,
    heard: &SpeechTimeline,
    speaker: Speaker,
    sink: &mut dyn TranscriptSink,
) -> Result<Option<Error>, Error> {
    let mut failure = None;
    for line in stdout.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Line>(&line) {
            Ok(Line::Final {
                start_sec,
                end_sec,
                text,
            }) => {
                // The sink owns formatting; this only owns normalization.
                let Some(text) = collapse_whitespace(&text) else {
                    continue;
                };
                let heard_speech = |start, end| heard.heard_speech(start, end);
                if !over_heard_speech(heard_speech, start_sec, end_sec, &text) {
                    continue;
                }
                sink.write(&Utterance {
                    // Truncating to whole seconds matches the [HH:MM:SS]
                    // line format; rounding would put an utterance a
                    // fraction before its own audio.
                    start_sec: start_sec.max(0.0) as u64,
                    speaker,
                    text,
                })?;
            }
            Ok(Line::Error { code, message }) => {
                failure = Some(Error::Sidecar(format!("{code}: {message}")));
            }
            Ok(
                Line::Volatile { .. }
                | Line::Progress { .. }
                | Line::Done { .. }
                | Line::Ready { .. },
            ) => {}
            Ok(Line::Probe(_)) => {}
            Err(e) => {
                // A malformed line is a contract violation, not something
                // to skip quietly — the sidecar promised JSON lines.
                return Err(Error::Sidecar(format!(
                    "unparseable line from meet-stt ({e}): {line}"
                )));
            }
        }
    }
    Ok(failure)
}

impl crate::SttEngine for AppleEngine {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn transcribe(
        &mut self,
        wav: &Path,
        speaker: Speaker,
        sink: &mut dyn TranscriptSink,
    ) -> Result<(), Error> {
        let mut child = Command::new(&self.binary)
            .arg(wav)
            .args(["--locale", &self.locale])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Sidecar(format!("could not run {}: {e}", self.binary.display())))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Sidecar("sidecar stdout was not captured".into()))?;

        // The silence gate's evidence, scored from the same file while the
        // sidecar reads it. [`SpeechTimeline`] explains why Apple is gated on
        // its output rather than its input. The file is streamed through in
        // blocks, never held whole.
        //
        // The order matters for which error the caller sees. The sidecar is
        // already running over the whole file, quiet or not, and every line it
        // prints is read below as before. So a missing locale or a broken
        // model is reported the same way for a silent meeting as for a
        // talkative one. Only a file the gate itself cannot read stops things
        // first. Nothing may be written without the gate, so there is no
        // honest way to go on, and the sidecar is stopped rather than left
        // blocked on a pipe nobody drains.
        let mut heard = SpeechTimeline::with_default_vad();
        if let Err(error) = crate::stream_wav_16k_mono(wav, |block| heard.push(block)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        heard.finish();

        let failure = write_batch_finals(BufReader::new(stdout), &heard, speaker, sink)?;

        let status = child.wait()?;
        if let Some(error) = failure {
            return Err(error);
        }
        if !status.success() {
            return Err(Error::Sidecar(format!("meet-stt exited with {status}")));
        }

        sink.flush()
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start_session(
        &mut self,
        options: SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn LiveListener>,
    ) -> Result<Box<dyn SttSession>, Error> {
        Ok(Box::new(AppleSession::spawn(
            &self.binary,
            &self.locale,
            options,
            sink,
            listener,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apple::testutil::{ROOM_TONE, ROOM_TONE_THEN_SPEECH, heard};

    #[test]
    fn a_batch_final_over_silence_never_reaches_the_sink() {
        let mut timeline = heard(0..0);
        timeline.finish();
        let mut sink = crate::CollectingSink::new();

        let failure =
            write_batch_finals(ROOM_TONE.as_bytes(), &timeline, Speaker::You, &mut sink).unwrap();

        assert!(failure.is_none());
        assert!(
            sink.utterances.is_empty(),
            "a final settled over quiet audio was written: {:?}",
            sink.lines()
        );
    }

    #[test]
    fn a_batch_final_over_speech_still_reaches_the_sink() {
        let mut timeline = heard(20..23);
        timeline.finish();
        let mut sink = crate::CollectingSink::new();

        write_batch_finals(
            ROOM_TONE_THEN_SPEECH.as_bytes(),
            &timeline,
            Speaker::You,
            &mut sink,
        )
        .unwrap();

        assert_eq!(sink.lines(), ["[00:00:20] You: About two days."]);
    }
}
