//! `transcription.live: false` (TUR-137): no engine while the meeting
//! records, then the whole meeting from its WAVs once it has stopped.
//!
//! This is the batch path the `stt` crate has always had
//! ([`stt::transcribe_meeting`]), run where live engines would have been
//! finished. It keeps the live path's promises: it never touches the
//! recording (the WAVs are closed before it starts), a failure is a sentence
//! on the status, and whatever reads `transcript.md` afterwards waits for it
//! through the same [`TranscriptFinal`].
//!
//! Stop waits for it only as long as it waits for live engines. A meeting
//! that needs longer is finished in the background with the status still
//! `running`, and the window hears `stopped` (or `failed`) when it ends. So
//! the board is not sealed at Stop here: this thread is not an abandoned
//! engine but the transcription itself. The next meeting's start still takes
//! the board from it.

use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stt::{MeetingPaths, transcribe_meeting};

use super::board::Scope;
use super::supervise::guarded;
use super::{OpenEngine, State, Status, TranscriptFinal};
use crate::lock::lock_or_recover;

/// Appended to every failure sentence here: the recording is over, and safe.
const AUDIO_SAVED: &str = "The recording itself is complete, and the meeting can be transcribed \
                           from its saved audio.";

/// Transcribe the meeting whose `transcript.md` is `transcript`, waiting up
/// to `timeout` for it. See [`super::Transcription::finish_final`].
///
/// `marks_late`: when the batch outlasts `timeout`, mark the meeting's audio
/// complete for retention once it ends cleanly, which the caller cannot do
/// from the `running` status it got back.
pub(super) fn finish(
    scope: Scope,
    transcript: PathBuf,
    open: OpenEngine,
    timeout: Duration,
    marks_late: bool,
) -> (Status, TranscriptFinal) {
    // Whether Stop has stopped waiting. Both sides decide under this lock, so
    // a batch that ends right at the timeout is handled by exactly one.
    let late = Arc::new(Mutex::new(false));
    let (done_tx, done) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-transcribe-after-stop".to_string())
        .spawn({
            let (scope, late) = (scope.clone(), Arc::clone(&late));
            move || {
                let status = transcribe(&scope, &transcript, open);
                let late = lock_or_recover(&late);
                if *late
                    && marks_late
                    && let Some(meeting_dir) = transcript.parent()
                {
                    crate::retention::transcript_finished(meeting_dir, &status);
                }
                // Under the lock, so Stop sees either "done" or "not yet".
                let _ = done_tx.send(());
                drop(late);
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "could not spawn the after-Stop transcription thread");
        scope.fail(format!(
            "The meeting could not be transcribed: {error}. {AUDIO_SAVED}"
        ));
        return (scope.seal(), final_now());
    }

    match done.recv_timeout(timeout) {
        Ok(()) => {}
        Err(RecvTimeoutError::Disconnected) => {
            if !scope.is_settled() {
                scope.fail(format!(
                    "Transcribing the meeting stopped unexpectedly. {AUDIO_SAVED}"
                ));
            }
        }
        Err(RecvTimeoutError::Timeout) => {
            let mut late = lock_or_recover(&late);
            if done.try_recv().is_err() {
                *late = true;
                drop(late);
                tracing::info!(
                    timeout_s = timeout.as_secs_f64(),
                    "the meeting is still being transcribed; it finishes in the background"
                );
                let status = scope
                    .with_board(|board| board.status.clone())
                    .unwrap_or_else(Status::idle);
                let pending = TranscriptFinal {
                    pending: Some(done),
                    patient: true,
                };
                return (status, pending);
            }
        }
    }
    (scope.seal(), final_now())
}

/// A [`TranscriptFinal`] for a file nothing will write to any more.
fn final_now() -> TranscriptFinal {
    TranscriptFinal {
        pending: None,
        patient: false,
    }
}

/// Open the engine, transcribe both WAVs into `transcript.md`, and settle the
/// status. Returns the status it ended with.
fn transcribe(scope: &Scope, transcript: &Path, open: OpenEngine) -> Status {
    let (engine, outcome) = run(scope, transcript, open);
    let ended = Status {
        state: if outcome.is_ok() {
            State::Stopped
        } else {
            State::Failed
        },
        engine: engine.map(str::to_string),
        detail: outcome.as_ref().err().cloned(),
    };
    match outcome {
        Ok(()) => scope.settle(),
        Err(detail) => scope.fail(detail),
    }
    // What the board says, unless the next meeting has taken it.
    scope
        .with_board(|board| board.status.clone())
        .unwrap_or(ended)
}

/// The work itself. Returns the engine's name once one opened, and a failure
/// sentence for the window if it did not get to the end.
fn run(
    scope: &Scope,
    transcript: &Path,
    open: OpenEngine,
) -> (Option<&'static str>, Result<(), String>) {
    let Some(meeting_dir) = transcript.parent() else {
        let detail = format!(
            "The meeting could not be transcribed: {} is not in a meeting folder. {AUDIO_SAVED}",
            transcript.display()
        );
        return (None, Err(detail));
    };
    // A panic while opening or transcribing is a bug in an engine, and still
    // only a transcription failure.
    let mut engine = match guarded(|| Ok(open())) {
        Ok(Ok(engine)) => engine,
        Ok(Err(reason)) | Err(stt::Error::Engine(reason)) => {
            let detail = format!("The meeting could not be transcribed: {reason}. {AUDIO_SAVED}");
            return (None, Err(detail));
        }
        Err(error) => {
            let detail = format!("The meeting could not be transcribed: {error}. {AUDIO_SAVED}");
            return (None, Err(detail));
        }
    };
    let name = engine.name();
    scope.running(name);
    tracing::info!(engine = name, "transcribing the meeting after Stop");
    let paths = MeetingPaths::new(meeting_dir);
    match guarded(|| transcribe_meeting(&paths, engine.as_mut())) {
        Ok(outcome) => {
            tracing::info!(
                engine = outcome.engine,
                lines = outcome.lines,
                "the meeting was transcribed after Stop"
            );
            (Some(name), Ok(()))
        }
        Err(error) => {
            let detail = format!("Transcribing the meeting failed: {error}. {AUDIO_SAVED}");
            (Some(name), Err(detail))
        }
    }
}

#[cfg(test)]
mod tests;
