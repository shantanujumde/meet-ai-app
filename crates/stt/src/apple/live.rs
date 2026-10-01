//! The live path: `meet-stt --stdin` fed from `feed`, read on its own thread.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::AppleEngine;
use super::protocol::{Line, over_heard_speech};
use crate::Error;
use crate::session::{LiveEmitter, LiveListener, SessionOptions, SessionOutcome, SttSession};
use crate::sink::TranscriptSink;
use crate::vad::{SAMPLE_RATE, SpeechTimeline};

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
pub(super) struct AppleSession {
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
    pub(super) fn spawn(
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
            // quality: allow-unwrap moved as-is; the lock is only poisoned if a holder panicked
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
            // quality: allow-unwrap moved as-is; `spawn` always sets the reader
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
                    // quality: allow-unwrap moved as-is; the lock is only poisoned if a holder panicked
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
    use crate::Speaker;
    use crate::apple::testutil::{ROOM_TONE, ROOM_TONE_THEN_SPEECH, heard};

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
