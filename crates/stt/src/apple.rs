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

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::session::{LiveEmitter, LiveListener, SessionOptions, SessionOutcome, SttSession};
use crate::sink::TranscriptSink;
use crate::vad::{SAMPLE_RATE, SpeechTimeline};
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

/// The Apple half of the silence gate: is this result over heard speech?
///
/// Logs when it says no, the way whisper logs a segment its own guard drops,
/// so a missing line can be traced to the gate instead of guessed at.
///
/// `heard_speech` is a closure rather than a `&SpeechTimeline` so the live
/// path can lock the shared timeline for the question alone. The guard is
/// gone before anything is logged, so a slow log subscriber never stalls
/// `feed` on the audio thread.
fn over_heard_speech(
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

/// A live Apple transcription: `meet-stt --stdin` fed from `feed`.
///
/// TUR-31 (stdin, per channel, two sidecars): the recorder tees one channel's
/// 16 kHz mono i16 samples onto this process's stdin exactly as captured, with
/// no header and no framing. Apple emits volatile/final natively over that
/// stream, which is the whole reason engine 1 needs no VAD chunking (SPEC
/// §2.5) — unlike [`crate::whisper::WhisperSession`], there is no
/// [`crate::session::SpanAssembler`] here; the analyzer decides utterance
/// boundaries itself.
///
/// It is still gated the way whisper is. `feed` scores every block with the
/// crate's detector *before* writing it to the pipe, into a [`SpeechTimeline`]
/// shared with the reader thread. The sidecar cannot settle audio it has not
/// read yet, so by the time a result arrives the detector has already heard
/// everything that result covers. A result over audio with no speech in it is
/// neither shown nor written. That is the room-tone `"I"` Apple settles over
/// quiet pink noise.
///
/// Reading and writing happen on different threads by construction: `feed`
/// writes to the child's stdin on the caller's thread — the same thread the
/// live tap owns — and must never block on anything the sidecar is slow to
/// produce. A dedicated reader thread drains stdout and drives the
/// [`LiveEmitter`], and `finish` joins it to collect the outcome. Nothing here
/// re-implements the tail contract, the coalescing, or `seq` — that is exactly
/// what routing through `LiveEmitter` buys.
pub struct AppleSession {
    child: Child,
    /// `None` after `finish` has taken it, which is what sends the sidecar EOF.
    stdin: Option<ChildStdin>,
    reader: Option<JoinHandle<Result<SessionOutcome, Error>>>,
    /// Written by `feed`, read by the reader thread to gate each result.
    heard: Arc<Mutex<SpeechTimeline>>,
    /// Tracked here, not in the reader thread, because `feed` is the only
    /// place that ever sees the sample count — the reader thread only sees
    /// text.
    samples_written: u64,
}

impl AppleSession {
    fn spawn(
        binary: &Path,
        locale: &str,
        options: SessionOptions,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn LiveListener>,
    ) -> Result<Self, Error> {
        let mut child = Command::new(binary)
            .args(["--stdin", "--locale", locale, "--volatile"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited rather than piped: nothing here drains stderr, and a
            // piped-but-undrained pipe is a deadlock waiting for the sidecar to
            // print more than 64 KB of diagnostics.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::Sidecar(format!("could not run {}: {e}", binary.display())))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Sidecar("sidecar stdin was not captured".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Sidecar("sidecar stdout was not captured".into()))?;
        let mut reader = BufReader::new(stdout);

        // Block for exactly one line before handing back a session. By the
        // time `main.swift` prints anything at all, `preflight` has already
        // resolved the engine, the locale and the model — so a start-up
        // failure (engine unavailable, locale not installed) always arrives as
        // this first line, never silently after. This is also why it is safe
        // to block: nothing before `ready` waits on stdin, so this can never
        // hang on a sidecar that is simply waiting for audio.
        let mut first_line = String::new();
        loop {
            first_line.clear();
            let read = reader
                .read_line(&mut first_line)
                .map_err(|e| Error::Sidecar(format!("reading meet-stt's startup line: {e}")))?;
            if read == 0 {
                let status = child.wait();
                return Err(Error::Sidecar(format!(
                    "meet-stt exited before printing a line ({status:?})"
                )));
            }
            if first_line.trim().is_empty() {
                continue;
            }
            break;
        }
        match serde_json::from_str::<Line>(first_line.trim()) {
            Ok(Line::Ready { .. }) => {}
            Ok(Line::Error { code, message }) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Sidecar(format!("{code}: {message}")));
            }
            Ok(other) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Sidecar(format!(
                    "expected a ready or error line from meet-stt --stdin, got {other:?}"
                )));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Sidecar(format!(
                    "unparseable startup line from meet-stt ({e}): {}",
                    first_line.trim()
                )));
            }
        }

        let heard = Arc::new(Mutex::new(SpeechTimeline::with_default_vad()));
        let reader = {
            let heard = Arc::clone(&heard);
            std::thread::spawn(move || read_live_lines(reader, &heard, options, sink, listener))
        };

        Ok(Self {
            child,
            stdin: Some(stdin),
            reader: Some(reader),
            heard,
            samples_written: 0,
        })
    }
}

impl SttSession for AppleSession {
    fn engine_name(&self) -> &'static str {
        AppleEngine::NAME
    }

    fn feed(&mut self, samples: &[i16]) -> Result<(), Error> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(Error::Sidecar("feed called after finish".into()));
        };

        // Scored before it is written, never after: this ordering is what
        // guarantees the gate has heard a result's audio before the result
        // can exist. It is the same detector work whisper's `feed` does on
        // this thread.
        self.heard
            .lock()
            .expect("speech timeline mutex")
            .push(samples);

        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        // A dead or wedged sidecar surfaces here as a typed error, never a
        // panic: `BrokenPipe` if the process has exited (Rust's SIGPIPE is
        // ignored by the runtime, so the write fails instead of killing us),
        // or whatever the OS reports if the pipe cannot take more right now.
        // The caller — not this crate — decides what "stop the session, keep
        // recording" means; this only has to report the failure honestly.
        stdin.write_all(&bytes).map_err(|e| {
            Error::Sidecar(format!(
                "meet-stt's stdin is gone, transcription stopped: {e}"
            ))
        })?;
        self.samples_written += samples.len() as u64;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<SessionOutcome, Error> {
        // Dropping stdin sends EOF, which is the stdin path's only signal that
        // the meeting ended (there is no WAV header to close).
        self.stdin.take();

        let mut outcome = self
            .reader
            .take()
            .expect("spawn always starts the reader thread")
            .join()
            .map_err(|_| Error::Sidecar("meet-stt's reader thread panicked".into()))??;
        outcome.audio_sec = self.samples_written / SAMPLE_RATE as u64;

        // The reader thread only returns after stdout hits EOF, which on a
        // normal finish happens once the process has already exited; this is
        // just reaping it, not waiting on anything new.
        let _ = self.child.wait();
        Ok(outcome)
    }
}

#[cfg(test)]
impl AppleSession {
    /// Simulates the sidecar dying mid-meeting, for the independence test
    /// below. `SIGKILL` rather than `kill -TERM`: this is standing in for a
    /// crash, not a graceful shutdown the sidecar could still flush before.
    fn kill_for_test(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Drain `meet-stt --stdin`'s stdout and drive the tail contract.
///
/// Runs on its own thread for the session's whole life. Returns once stdout
/// hits EOF, which happens either because the sidecar finished normally (a
/// `done` line, then exit) or because it died (the pipe just closes) — the
/// two are told apart by whether a `done` line was ever seen.
///
/// Every final and every volatile passes the silence gate first (see
/// [`AppleSession`]). A dropped final is simply never written. A dropped
/// volatile is never shown, so a silent meeting does not flash a phantom word
/// in the pane either.
///
/// Generic over the reader so the gate can be tested with lines written by
/// hand, without a sidecar.
fn read_live_lines(
    reader: impl BufRead,
    heard: &Mutex<SpeechTimeline>,
    options: SessionOptions,
    mut sink: Box<dyn TranscriptSink + Send>,
    listener: Box<dyn LiveListener>,
) -> Result<SessionOutcome, Error> {
    let speaker = options.speaker;
    let mut emitter = LiveEmitter::new(&options, listener);
    let mut failure = None;
    let mut saw_done = false;

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                failure = Some(Error::Io(e));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let gate = |start_sec, end_sec, text: &str| {
            // The guard is a temporary, dropped as soon as the answer is in.
            let heard_speech = |start, end| {
                heard
                    .lock()
                    .expect("speech timeline mutex")
                    .heard_speech(start, end)
            };
            over_heard_speech(heard_speech, start_sec, end_sec, text)
        };
        match serde_json::from_str::<Line>(&line) {
            Ok(Line::Final {
                start_sec,
                end_sec,
                text,
            }) => {
                if !gate(start_sec, end_sec, &text) {
                    // A kept final would have cleared the tail, so a dropped
                    // one must too, or its hypothesis would stay on screen.
                    emitter.withdraw();
                    continue;
                }
                if let Err(e) = emitter.finalize(start_sec, &text, sink.as_mut()) {
                    failure = Some(e);
                    break;
                }
            }
            Ok(Line::Volatile {
                start_sec,
                end_sec,
                text,
            }) => {
                if gate(start_sec, end_sec, &text) {
                    emitter.volatile(start_sec, &text);
                }
            }
            Ok(Line::Done { .. }) => {
                saw_done = true;
            }
            Ok(Line::Error { code, message }) => {
                failure = Some(Error::Sidecar(format!("{code}: {message}")));
                break;
            }
            Ok(Line::Probe(_) | Line::Progress { .. } | Line::Ready { .. }) => {}
            Err(e) => {
                failure = Some(Error::Sidecar(format!(
                    "unparseable line from meet-stt ({e}): {line}"
                )));
                break;
            }
        }
    }

    // Whatever was still a guess when the stream ended is thrown away, never
    // promoted — the same rule every engine's `finish()` follows, and the pane
    // is told so the tail cannot sit on screen forever.
    let discarded_volatile = emitter.withdraw();
    let finalized = emitter.finalized();

    if let Some(error) = failure {
        return Err(error);
    }
    sink.flush()?;
    if !saw_done {
        return Err(Error::Sidecar(
            "meet-stt's stream ended without a done line".into(),
        ));
    }

    Ok(SessionOutcome {
        speaker,
        finalized,
        discarded_volatile,
        // The caller fills this in from the sample count it tracked itself.
        audio_sec: 0,
        engine: AppleEngine::NAME,
    })
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

    // --- the silence gate, without a sidecar ---
    //
    // The lines below are the sidecar's real output shape. The room-tone one is
    // copied from `meet-stt room-tone-30s.wav --volatile` on macOS 27.0, run
    // against the seeded fixture (`seed=1` in `generate.sh`). The
    // detector is scripted, because what is under test is the wiring: that
    // Apple's results are held against the detector at all, on both paths.

    /// Scores 0.9 for frames inside `speech`, 0.0 elsewhere.
    struct SpeechAt {
        speech: std::ops::Range<usize>,
        next: usize,
    }

    impl crate::vad::Vad for SpeechAt {
        fn score(&mut self, _frame: &[i16]) -> f32 {
            let score = if self.speech.contains(&self.next) {
                0.9
            } else {
                0.0
            };
            self.next += 1;
            score
        }

        fn reset(&mut self) {
            self.next = 0;
        }
    }

    const FRAMES_PER_SEC: usize = SAMPLE_RATE as usize / crate::vad::FRAME_SAMPLES;

    /// 30 s of audio, with speech only in the given whole seconds.
    fn heard(speech_secs: std::ops::Range<usize>) -> SpeechTimeline {
        let vad = SpeechAt {
            speech: speech_secs.start * FRAMES_PER_SEC..speech_secs.end * FRAMES_PER_SEC,
            next: 0,
        };
        let mut timeline = SpeechTimeline::new(crate::vad::SegmentConfig::default(), Box::new(vad));
        timeline.push(&vec![0; SAMPLE_RATE as usize * 30]);
        timeline
    }

    const ROOM_TONE: &str = r#"{"type":"volatile","start_sec":0,"end_sec":30,"text":"I"}
{"type":"final","start_sec":0,"end_sec":3.84,"text":"I"}
{"type":"done","duration_sec":30}
"#;

    const ROOM_TONE_THEN_SPEECH: &str = r#"{"type":"final","start_sec":0,"end_sec":3.84,"text":"I"}
{"type":"volatile","start_sec":20.0,"end_sec":21.0,"text":"about two"}
{"type":"final","start_sec":20.0,"end_sec":23.4,"text":"About two days."}
{"type":"done","duration_sec":30}
"#;

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

    #[test]
    fn a_live_final_over_silence_is_neither_written_nor_shown() {
        let sink = crate::session::SharedCollector::new();
        let seen = crate::session::CollectingListener::new();
        let outcome = read_live_lines(
            ROOM_TONE.as_bytes(),
            &Mutex::new(heard(0..0)),
            SessionOptions::new(Speaker::You).with_volatile_per_sec(f64::INFINITY),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
        )
        .unwrap();

        assert_eq!(outcome.finalized, 0);
        assert!(
            sink.is_empty(),
            "quiet audio reached disk: {:?}",
            sink.lines()
        );
        assert!(
            seen.updates().is_empty(),
            "the pane was shown something for quiet audio: {:?}",
            seen.updates()
        );
    }

    #[test]
    fn a_live_final_over_speech_is_written_and_shown() {
        let sink = crate::session::SharedCollector::new();
        let seen = crate::session::CollectingListener::new();
        let outcome = read_live_lines(
            ROOM_TONE_THEN_SPEECH.as_bytes(),
            &Mutex::new(heard(20..23)),
            SessionOptions::new(Speaker::You).with_volatile_per_sec(f64::INFINITY),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
        )
        .unwrap();

        assert_eq!(outcome.finalized, 1);
        assert_eq!(sink.lines(), ["[00:00:20] You: About two days."]);
        let volatiles: Vec<_> = seen.volatiles().into_iter().map(|l| l.text).collect();
        assert_eq!(volatiles, ["about two"]);
        assert_eq!(seen.tail_for(Speaker::You), None);
    }

    #[test]
    fn a_dropped_live_final_still_clears_the_tail_it_settles() {
        // A hypothesis over real speech whose final lands entirely outside it
        // is contrived, but a tail left on screen is the failure either way.
        let lines = r#"{"type":"volatile","start_sec":20.0,"end_sec":21.0,"text":"about"}
{"type":"final","start_sec":25.0,"end_sec":26.0,"text":"I"}
{"type":"done","duration_sec":30}
"#;
        let seen = crate::session::CollectingListener::new();
        let sink = crate::session::SharedCollector::new();
        read_live_lines(
            lines.as_bytes(),
            &Mutex::new(heard(20..22)),
            SessionOptions::new(Speaker::You).with_volatile_per_sec(f64::INFINITY),
            Box::new(sink.clone()),
            Box::new(seen.clone()),
        )
        .unwrap();

        assert!(sink.is_empty());
        assert!(matches!(
            seen.updates().as_slice(),
            [
                crate::LiveUpdate::Volatile(_),
                crate::LiveUpdate::Dropped { .. }
            ]
        ));
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

    /// TUR-33's independence property: a transcription failure must not kill
    /// the recording. This cannot prove the capture side (a different crate)
    /// keeps running, but it proves the half that lives here — `feed` reports
    /// the sidecar's death as a typed [`Error`], never a panic and never a
    /// silent success — which is what lets a caller detach and carry on.
    #[test]
    fn a_dying_sidecar_surfaces_a_typed_feed_error_not_a_panic() {
        let Some(binary) = AppleEngine::discover() else {
            eprintln!("SKIPPED: target/meet-stt is not built — run `just sidecar`");
            return;
        };

        let mut session = match AppleSession::spawn(
            &binary,
            "en-US",
            SessionOptions::new(Speaker::You),
            Box::new(crate::CollectingSink::new()),
            Box::new(crate::NoListener),
        ) {
            Ok(session) => session,
            Err(e) => {
                eprintln!(
                    "SKIPPED: could not start a live apple session ({e}) — is en-US \
                     installed? (`meet-stt --install-locale --locale en-US`)"
                );
                return;
            }
        };

        // Enough real writes that the session is unambiguously alive before it
        // gets killed out from under itself.
        let block = vec![0i16; 1_600];
        for _ in 0..5 {
            session.feed(&block).expect("the sidecar is alive");
        }

        session.kill_for_test();

        // Killing a process is not synchronous with every fd it held becoming
        // unwritable, so give the OS a moment and retry rather than asserting
        // on the very next write.
        let mut saw_error = false;
        for _ in 0..40 {
            if session.feed(&block).is_err() {
                saw_error = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            saw_error,
            "feed must surface a typed error once the sidecar is dead, not hang \
             or silently succeed"
        );

        // finish() must stay honest about the failure too, rather than
        // reporting a clean outcome for a session whose sidecar was killed.
        match Box::new(session).finish() {
            Err(_) => {}
            Ok(outcome) => panic!(
                "finish() reported success ({outcome:?}) for a session whose \
                 sidecar was killed"
            ),
        }
    }
}
