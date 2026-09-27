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

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::sink::TranscriptSink;
use crate::{Error, Speaker, Utterance, collapse_whitespace};

/// The sidecar's answer to `--probe`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Probe {
    /// Is `SpeechTranscriber` present at all? False below macOS 26.
    pub available: bool,
    /// Is the on-device model for the requested locale already downloaded?
    ///
    /// This is the one that decides whether transcription can run offline.
    /// `available && !installed` means the engine exists but would need a
    /// network fetch first, which is a setup step, never a transcription step.
    #[serde(default)]
    pub installed: bool,
    /// The locale actually resolved, e.g. `en-US`. `None` if unsupported.
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub os_version: Option<String>,
}

impl Probe {
    /// Can this engine transcribe right now, with the network off?
    pub fn is_usable_offline(&self) -> bool {
        self.available && self.installed
    }
}

/// One line of the sidecar's stdout.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Line {
    Probe(Box<Probe>),
    Final {
        start_sec: f64,
        #[serde(default)]
        text: String,
    },
    /// Volatile results are UI-only (SPEC §2.5) and are dropped here rather
    /// than being handed to a sink that would persist them.
    Volatile {
        #[allow(dead_code)]
        start_sec: f64,
        #[allow(dead_code)]
        #[serde(default)]
        text: String,
    },
    Progress {
        #[allow(dead_code)]
        fraction: f64,
    },
    Done {
        #[allow(dead_code)]
        #[serde(default)]
        duration_sec: Option<f64>,
    },
    Error {
        code: String,
        message: String,
    },
}

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

        let mut failure = None;
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Line>(&line) {
                Ok(Line::Final { start_sec, text }) => {
                    // The sink owns formatting; this only owns normalization.
                    let Some(text) = collapse_whitespace(&text) else {
                        continue;
                    };
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
                Ok(Line::Volatile { .. } | Line::Progress { .. } | Line::Done { .. }) => {}
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

        let status = child.wait()?;
        if let Some(error) = failure {
            return Err(error);
        }
        if !status.success() {
            return Err(Error::Sidecar(format!("meet-stt exited with {status}")));
        }

        sink.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_line_parses() {
        let line = r#"{"type":"probe","engine":"apple-speech","available":true,
            "installed":true,"locale":"en-US","installed_locales":["en-US"],
            "os_version":"Version 27.0"}"#;
        let Line::Probe(probe) = serde_json::from_str::<Line>(line).unwrap() else {
            panic!("expected a probe line");
        };
        assert!(probe.is_usable_offline());
        assert_eq!(probe.locale.as_deref(), Some("en-US"));
    }

    #[test]
    fn an_available_but_uninstalled_engine_is_not_usable_offline() {
        let probe = Probe {
            available: true,
            installed: false,
            locale: Some("en-US".into()),
            reason: None,
            os_version: None,
        };
        assert!(!probe.is_usable_offline());
    }

    #[test]
    fn a_final_line_parses() {
        let line =
            r#"{"type":"final","start_sec":6.9,"end_sec":8.9,"text":"Sessions are the blocker."}"#;
        match serde_json::from_str::<Line>(line).unwrap() {
            Line::Final { start_sec, text } => {
                assert_eq!(start_sec, 6.9);
                assert_eq!(text, "Sessions are the blocker.");
            }
            other => panic!("expected a final line, got {other:?}"),
        }
    }

    #[test]
    fn an_error_line_parses() {
        let line = r#"{"type":"error","code":"locale_not_installed","message":"nope"}"#;
        match serde_json::from_str::<Line>(line).unwrap() {
            Line::Error { code, .. } => assert_eq!(code, "locale_not_installed"),
            other => panic!("expected an error line, got {other:?}"),
        }
    }
}
