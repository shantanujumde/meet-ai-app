//! Ending a recording: the user's Stop, a tick that failed (TUR-97), and a
//! ticker that never spawned. Moved out of `recording.rs` (TUR-161) so all
//! three end the same way, through [`close`]:
//!
//! 1. stop the session, so the WAVs and `segments.json` are final;
//! 2. finish the transcript, with the after-Stop handling when the meeting
//!    is kept ([`Ending::after_stop`]);
//! 3. end, with or without an error.
//!
//! Before TUR-161 a recording cut short by a disk error or a failed device
//! reopen skipped step 2's after-Stop handling, so it got no notes run and no
//! `Transcribing` busy marker for retention, even though its audio was saved.

use tauri::AppHandle;

use super::{Phase, Recorder, RecordingSession, Status, Ticker, emit_state, interrupted, ticker};
use crate::error::UiError;
use crate::live_transcript::{self, Transcription};

/// How a recording came to end. The one thing [`close`] needs to know to
/// decide what happens to the meeting's transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    /// The user stopped it: the button, ⌘⇧R or the menu bar.
    Stopped,
    /// It stopped on its own because a tick failed: a disk error, a failed
    /// device reopen, a panic (TUR-97). The audio up to then is saved.
    Interrupted,
    /// The ticker thread never spawned, so the recording never started. Its
    /// folder is about to be removed (`Recorder::fail_start`).
    NeverStarted,
}

impl Ending {
    /// Whether the meeting gets the after-Stop handling: the `Transcribing`
    /// busy marker for retention and, with `agent.auto_run` on, the notes run
    /// (`agent_run::finish_then_run`). The one place this is decided.
    ///
    /// An interrupted recording does (TUR-161): its audio was saved, and
    /// notes from part of a meeting are better than none. A start that never
    /// happened does not: its folder is about to be deleted.
    pub(super) fn after_stop(self) -> bool {
        match self {
            Ending::Stopped | Ending::Interrupted => true,
            Ending::NeverStarted => false,
        }
    }
}

/// The parts of [`close`] that touch the app, so its order and its decision
/// are testable without an `AppHandle` or Core Audio.
pub(super) trait Close {
    type Session;
    type Transcript;
    /// Close the session's files.
    fn stop_session(&self, session: Self::Session) -> Result<(), String>;
    /// Finish the transcript: with the after-Stop handling, or just finish it.
    fn finish_transcript(&self, transcript: Self::Transcript, after_stop: bool);
}

/// The app's [`Close`]: the real session and transcript.
pub(super) struct Live<'a>(pub(super) &'a AppHandle);

impl Close for Live<'_> {
    type Session = RecordingSession;
    type Transcript = Transcription;

    fn stop_session(&self, session: RecordingSession) -> Result<(), String> {
        session.stop().map(|_report| ())
    }

    fn finish_transcript(&self, transcription: Transcription, after_stop: bool) {
        if after_stop {
            crate::agent_run::finish_then_run(self.0, transcription);
        } else {
            let _ = transcription.finish(live_transcript::STOP_TIMEOUT);
        }
    }
}

/// End a recording whose session is already out of its ticker: stop the
/// session (`None` when it was lost with the ticker), then finish the
/// transcript, then `end` with how stopping went (`None` when there was no
/// session to stop).
///
/// Audio first: it is the part that cannot be redone. Only once the WAVs are
/// closed, which also means every frame is in the tees, is the transcript
/// given its bounded chance to catch up. Its outcome reaches the window as a
/// `transcript://status` event, never as an error here: a short transcript is
/// not a failed recording.
///
/// A panic while stopping is an error, never an unwind past this: the
/// recorder would be left in `Stopping`, where every toggle is ignored.
pub(super) fn close<C: Close, R>(
    closing: &C,
    session: Option<C::Session>,
    transcript: Option<C::Transcript>,
    ending: Ending,
    end: impl FnOnce(Option<Result<(), String>>) -> R,
) -> R {
    let stopped = session.map(|session| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            closing.stop_session(session)
        }))
        .unwrap_or_else(|payload| {
            Err(format!(
                "stopping panicked: {}",
                ticker::panic_message(&*payload)
            ))
        })
    });
    if let Some(transcript) = transcript {
        closing.finish_transcript(transcript, ending.after_stop());
    }
    end(stopped)
}

impl Recorder {
    pub fn stop(&self, app: &AppHandle) -> Result<Status, UiError> {
        let (ticker, transcription) = match self.claim_stopping(app) {
            Err(status) => return Ok(status),
            Ok(taken) => taken,
        };
        let (session, lost) = take_session(ticker);
        close(
            &Live(app),
            session,
            transcription,
            Ending::Stopped,
            |stopped| {
                let message = match (lost, stopped) {
                    (Some(message), _) => Some(message),
                    (None, Some(Err(message))) => {
                        tracing::warn!(%message, "recording did not stop cleanly");
                        Some(message)
                    }
                    (None, _) => None,
                };
                // On the status as well as the return value: a stop from
                // ⌘⇧R or the menu bar has no caller that shows the error.
                let error = message.map(|message| UiError::app("recorder-failed", message));
                let status = self.transition(app, |inner| inner.end(error.clone()));
                error.map_or(Ok(status), Err)
            },
        )
    }

    /// Claim `Recording -> Stopping` and take the session (inside its ticker)
    /// and its transcript with it, or report the phase that beat us to it.
    #[allow(clippy::type_complexity)]
    fn claim_stopping(
        &self,
        app: &AppHandle,
    ) -> Result<(Option<Ticker<RecordingSession>>, Option<Transcription>), Status> {
        let mut inner = self.lock();
        if inner.status.phase != Phase::Recording {
            return Err(inner.status.clone());
        }
        inner.status.phase = Phase::Stopping;
        let ticker = inner.ticker.take();
        let transcription = inner.transcription.take();
        let status = inner.status.clone();
        drop(inner);
        emit_state(app, &status);
        Ok((ticker, transcription))
    }

    /// What a failed `tick()` means (TUR-97): the recording is over. Stop the
    /// session so what is on disk is finalised, finish the transcript as a
    /// Stop would (notes run included, TUR-161), go back to `Idle`, and tell
    /// the user — never keep showing `Recording` over a session that has
    /// stopped checkpointing.
    ///
    /// Why stop rather than log and keep ticking: every error `tick()` returns
    /// leaves the session unable to carry on. A checkpoint error is a failed
    /// fsync, header patch or `segments.json` write, or the microphone having
    /// stopped delivering audio — retrying the same disk or device five
    /// seconds later is not a plan, and ticking on would mean a recording the
    /// UI calls live while nothing on disk is being made crash-safe. A reopen
    /// error is worse: `reopen_segment` stops both channels before it rebuilds
    /// them, so a failure part-way leaves the session holding stopped sources,
    /// capturing nothing. `meet-rec` agrees that a tick error ends the
    /// recording (its loop `?`s out of it), but it drops the session without
    /// `stop()`, abandoning the header patch; the app stops it instead. That
    /// is safe after either kind of failure: both sources' `stop()` are
    /// idempotent (a second call on a stopped channel re-syncs and re-patches
    /// what is there), so this still writes the contract §7 graceful-stop
    /// anchor for whatever audio did land.
    ///
    /// Runs on the ticker thread, so it must never join that thread: it takes
    /// its own ticker out of `Inner` and detaches it. If a user's stop has
    /// already claimed `Stopping` — it is blocked joining this thread — the
    /// session is handed back through the join so that stop finishes it
    /// exactly as it would have without the failure.
    pub(super) fn fail_mid_recording(
        &self,
        app: &AppHandle,
        session: RecordingSession,
        message: String,
    ) -> Option<RecordingSession> {
        let Some((stopping, transcription)) = self.claim_interrupted(&message) else {
            return Some(session);
        };
        emit_state(app, &stopping);
        close(
            &Live(app),
            Some(session),
            transcription,
            Ending::Interrupted,
            |stopped| {
                let stop_error = match &stopped {
                    Some(Err(stop_message)) => {
                        tracing::warn!(message = %stop_message, "the failed recording did not stop cleanly");
                        Some(stop_message.as_str())
                    }
                    _ => None,
                };
                let (idle, error) = self.end_interrupted(&message, stop_error);
                emit_state(app, &idle);
                // The same `UiError` the idle status carries, so the window
                // and the notification cannot tell two different stories.
                // It goes out because the window may be hidden, which is the
                // normal case for a recording started with ⌘⇧R.
                crate::notify::interrupted(app, &error.message);
            },
        );
        None
    }

    /// The first half of [`Recorder::fail_mid_recording`]: claim
    /// `Recording -> Stopping` for a failed tick and take what the stop needs.
    /// `None` means a user's stop got there first and owns the ending.
    ///
    /// Split out (with [`Recorder::end_interrupted`]) so the state changes a
    /// failed tick makes are testable on a real `Ticker` thread without an
    /// `AppHandle` or Core Audio.
    fn claim_interrupted(&self, message: &str) -> Option<(Status, Option<Transcription>)> {
        let mut inner = self.lock();
        if inner.status.phase != Phase::Recording {
            tracing::error!(
                %message,
                "a recording tick failed while it was already being stopped"
            );
            return None;
        }
        tracing::error!(%message, "a recording tick failed; stopping the recording");
        inner.status.phase = Phase::Stopping;
        // This runs on the ticker's own thread, which must never join itself.
        if let Some(own) = inner.ticker.take() {
            own.detach();
        }
        let transcription = inner.transcription.take();
        Some((inner.status.clone(), transcription))
    }

    /// The second half: back to `Idle` with the reason on the status, once
    /// the session is stopped (`stop_error` is how that went). Returns the
    /// idle status to announce and the error it carries.
    fn end_interrupted(&self, message: &str, stop_error: Option<&str>) -> (Status, UiError) {
        let error = interrupted::interrupted(message, stop_error);
        let mut inner = self.lock();
        inner.end(Some(error.clone()));
        (inner.status.clone(), error)
    }
}

/// End the ticker and take the session back out of it, *before* the
/// session's own stop (TUR-97). The session only comes back out of the join,
/// so no tick can be running, or start, once we hold it — a checkpoint can
/// never race the final header patch and `segments.json` write. The wait is
/// at most one tick already in flight; the thread is woken, not polled.
///
/// Returns the session, or `None` with, when the ticker died, why.
fn take_session(
    ticker: Option<Ticker<RecordingSession>>,
) -> (Option<RecordingSession>, Option<String>) {
    let Some(ticker) = ticker else {
        tracing::error!("phase was Recording with no session attached; recovering to idle");
        return (None, None);
    };
    match ticker.stop() {
        Ok(Some(session)) => (Some(session), None),
        Ok(None) => {
            // Only a failed tick's handler keeps the session, and it only
            // does so after claiming `Stopping` itself — which would have
            // made our claim lose. Unreachable in practice; recover rather
            // than stick in `Stopping`.
            tracing::error!("the recording ticker kept the session; recovering to idle");
            (None, None)
        }
        Err(message) => {
            tracing::error!(%message, "the recording ticker died; the session went with it");
            (None, Some(message))
        }
    }
}

#[cfg(test)]
mod tests;
