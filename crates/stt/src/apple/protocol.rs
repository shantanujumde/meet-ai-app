//! What the `meet-stt` sidecar prints: one JSON object per line.
//!
//! Everything here is documented at the top of `sidecar/meet-stt/main.swift`;
//! the two move together.

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
pub(super) enum Line {
    Probe(Box<Probe>),
    Final {
        start_sec: f64,
        /// Where the audio this result claims ends. The silence gate checks
        /// the whole `[start_sec, end_sec]` range against the detector. A
        /// missing end only narrows the check to the start instant.
        #[serde(default)]
        end_sec: Option<f64>,
        #[serde(default)]
        text: String,
    },
    /// Volatile results are UI-only (SPEC §2.5). The batch path drops them
    /// here rather than handing them to a sink that would persist them.
    Volatile {
        start_sec: f64,
        #[serde(default)]
        end_sec: Option<f64>,
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
    /// `--stdin` only: the model is loaded, the locale is reserved, and the
    /// analyzer is now pulling from stdin. Emitted once, before any audio has
    /// necessarily been read — see `sidecar/meet-stt/main.swift`'s note on why
    /// a start-up failure always arrives *before* this line, never after.
    Ready {
        #[allow(dead_code)]
        locale: String,
        #[allow(dead_code)]
        sample_rate: f64,
        #[allow(dead_code)]
        #[serde(default)]
        analyzer_sample_rate: Option<f64>,
    },
}

/// The Apple half of the silence gate: is this result over heard speech?
///
/// Logs when it says no, the way whisper logs a segment its own guard drops,
/// so a missing line can be traced to the gate instead of guessed at.
///
/// `heard_speech` is a closure rather than a `&SpeechTimeline` so the live
/// path can lock the shared timeline for the question alone. The guard is
/// gone before anything is logged, so a slow log subscriber never stalls
/// `feed` on the audio thread.
pub(super) fn over_heard_speech(
    heard_speech: impl FnOnce(f64, f64) -> bool,
    start_sec: f64,
    end_sec: Option<f64>,
    text: &str,
) -> bool {
    let end_sec = end_sec.unwrap_or(start_sec);
    let kept = heard_speech(start_sec, end_sec);
    if !kept {
        tracing::debug!(
            text,
            start_sec,
            end_sec,
            "dropped: the detector heard no speech under this result"
        );
    }
    kept
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
            Line::Final {
                start_sec,
                end_sec,
                text,
            } => {
                assert_eq!(start_sec, 6.9);
                assert_eq!(end_sec, Some(8.9));
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

    #[test]
    fn a_ready_line_parses() {
        let line = r#"{"type":"ready","locale":"en-US","sample_rate":16000,
            "analyzer_sample_rate":16000}"#;
        match serde_json::from_str::<Line>(line).unwrap() {
            Line::Ready { locale, .. } => assert_eq!(locale, "en-US"),
            other => panic!("expected a ready line, got {other:?}"),
        }
    }
}
