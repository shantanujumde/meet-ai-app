//! The recording lifecycle: start, stop, and what ⌘⇧R does.
//!
//! The recorder behind this is real (TUR-94's `audio::session::RecordingSession`):
//! start opens the microphone and, where available, the system-audio tap and
//! writes into the SPEC §3.1 meeting folder this module creates; stop closes
//! both cleanly and leaves `mic.wav`, `system.wav` and `segments.json` complete,
//! with no later repair step needed.
//!
//! Starting and stopping both take real wall-clock time — SPEC §8.1's
//! positive-control permission measurement, then Core Audio warming up each
//! channel — so every entry point here is meant to be called off a thread that
//! must stay responsive. `commands::toggle_recording`/`stop_recording` run this
//! on a blocking thread the same way `commands::permission_status` does; the
//! ⌘⇧R handler in `lib.rs` gives it a worker thread of its own so the shortcut
//! callback never blocks. Nothing here needs to know which caller it is: the
//! phase check and transition happen inside one `Mutex`, so whichever caller's
//! lock lands first wins a race and the other sees `Starting`/`Stopping` and
//! no-ops, rather than racing a second tap open.
//!
//! While a recording is live, a ticker thread ([`ticker`], TUR-97) owns the
//! `RecordingSession` and calls its `tick()` every
//! [`audio::session::TICK_INTERVAL`], which is what checkpoints the WAV headers
//! and `segments.json` every five seconds and reopens a segment on a default
//! device change. The `Mutex` holds that ticker instead of the bare session, so
//! the phase and the thing it owns still change together under one lock, and
//! the lock is never held across a tick.

mod ticker;

use std::sync::Mutex;

use audio::AudioSource;
use audio::session::RecordingSession;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};

use self::ticker::Ticker;

use crate::error::UiError;
use crate::permission;

/// The Tauri event the frontend subscribes to. Every transition emits one, so
/// the UI never has to poll and the menu bar, the titlebar and the sidebar all
/// see the same change at the same time.
pub const STATE_EVENT: &str = "recording://state";

/// Emitted with a [`UiError`] when a recording ends on its own because a tick
/// failed (TUR-97) — the one recorder failure with no command call waiting to
/// return it. `STATE_EVENT` still carries the move back to `Idle`; this says
/// why. A system notification goes out as well, because the window may be
/// hidden, which is the normal case for a recording started with ⌘⇧R.
pub const ERROR_EVENT: &str = "recording://error";

/// The ticker thread's name, so it is identifiable in a sample or a crash
/// report next to `meet-ai-record-shortcut`.
const TICKER_THREAD_NAME: &str = "meet-ai-recording-ticker";

/// Where the recorder is right now.
///
/// `Starting` and `Stopping` are not decoration: opening the tap (and, on
/// start, the SPEC §8.1 permission measurement ahead of it) and flushing the
/// last WAV header both take long enough to see, and a shortcut pressed twice
/// in that window must be ignored rather than queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Idle,
    Starting,
    Recording,
    Stopping,
}

/// The recorder's state as the webview sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    /// The folder name of the meeting being recorded, `None` when idle.
    pub meeting_id: Option<String>,
    /// Unix epoch milliseconds the recording started, so the UI can run its own
    /// timer instead of being fed a tick per second over IPC.
    pub started_at_ms: Option<i64>,
}

impl Status {
    fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            meeting_id: None,
            started_at_ms: None,
        }
    }
}

/// The recorder's status plus the live capture session behind it, one Mutex
/// so a phase transition and the session it owns can never observe each other
/// half-updated.
///
/// The session sits inside its [`Ticker`] rather than here directly (TUR-97):
/// the ticker thread owns it while recording and hands it back through the
/// join on stop. `Some` exactly when the phase is `Recording` — set in the
/// same critical section as the `Starting -> Recording` move, and taken in the
/// same one as `Recording -> Stopping`.
struct Inner {
    status: Status,
    ticker: Option<Ticker<RecordingSession>>,
}

impl Inner {
    fn idle() -> Self {
        Self {
            status: Status::idle(),
            ticker: None,
        }
    }
}

/// Managed Tauri state. One recorder per app, because two would fight over the
/// system audio tap — the same reason the single-instance plugin is wired up.
pub struct Recorder {
    inner: Mutex<Inner>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::idle()),
        }
    }
}

impl Recorder {
    pub fn status(&self) -> Status {
        self.lock().status.clone()
    }

    /// A poisoned lock here means a previous call panicked while holding it.
    /// Recovering is strictly better than taking the whole app down; the worst
    /// case is a stuck phase, not a half-broken invariant.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Mutate the state and tell the whole app about it in one step, so a
    /// transition can never land without an event.
    fn transition(&self, app: &AppHandle, change: impl FnOnce(&mut Inner)) -> Status {
        let next = {
            let mut inner = self.lock();
            change(&mut inner);
            inner.status.clone()
        };
        // A webview that has gone away is not an error worth propagating up
        // into a recording control.
        if let Err(error) = app.emit(STATE_EVENT, &next) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        next
    }

    /// Start if idle, stop if recording, and do nothing mid-transition.
    ///
    /// One entry point for the button, the menu item and the global shortcut,
    /// so all three cannot disagree about what "toggle" means.
    pub fn toggle(&self, app: &AppHandle) -> Result<Status, UiError> {
        match self.status().phase {
            Phase::Idle => self.start(app),
            Phase::Recording => self.stop(app),
            // Mid-transition. Returning the current state rather than an error
            // is correct: the user pressed a toggle and the answer is "already
            // doing that", which is not a failure worth interrupting them for.
            Phase::Starting | Phase::Stopping => Ok(self.status()),
        }
    }

    /// Claim `Idle -> Starting`, or report the phase that beat us to it.
    fn claim_starting(&self, app: &AppHandle) -> Result<(), Status> {
        let mut inner = self.lock();
        if inner.status.phase != Phase::Idle {
            return Err(inner.status.clone());
        }
        inner.status.phase = Phase::Starting;
        let status = inner.status.clone();
        drop(inner);
        if let Err(error) = app.emit(STATE_EVENT, &status) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        Ok(())
    }

    pub fn start(&self, app: &AppHandle) -> Result<Status, UiError> {
        if let Err(status) = self.claim_starting(app) {
            return Ok(status);
        }

        // SPEC §8.1: a denied tap returns `noErr` and delivers bit-exact
        // zeros, so a return code cannot tell us anything — only the positive
        // control (a tone played and listened for) can. The UI already keeps
        // the controls disabled while permission is absent; this measurement
        // is the backstop for the one path that has no button to disable, the
        // global shortcut firing with the window unfocused or hidden.
        if permission::measure().state == permission::State::Denied {
            return Err(self.fail_start(
                app,
                None,
                UiError::app(
                    "permission-denied",
                    "meet-ai is not allowed to record this Mac's audio, so starting a recording \
                     would capture nothing but silence.",
                ),
            ));
        }

        let started = chrono::Local::now();
        let id = meeting_id(started);
        if let Err(error) = create_meeting_folder(&id) {
            return Err(self.fail_start(app, Some(&id), error));
        }

        let audio_dir = match crate::meetings::root() {
            Ok(root) => root.join(&id).join("audio"),
            Err(error) => return Err(self.fail_start(app, Some(&id), error)),
        };

        let mic: Box<dyn AudioSource> = Box::new(audio::mic::MicSource::new());
        let sys = audio::session::default_system_source();

        match RecordingSession::start(audio_dir, mic, sys) {
            Ok(session) => self
                .enter_recording(app, &id, started, session)
                .map_err(|error| self.fail_start(app, Some(&id), error)),
            Err(message) => {
                Err(self.fail_start(app, Some(&id), UiError::app("recorder-failed", message)))
            }
        }
    }

    /// `Starting -> Recording`, with the ticker thread spawned and stored in
    /// the same critical section (TUR-97).
    ///
    /// Spawning under the lock is what keeps "phase is `Recording`" and "a
    /// ticker exists" one fact: the thread's first tick is an interval away,
    /// and its failure path ([`Recorder::fail_mid_recording`]) takes this lock
    /// before looking at the phase, so it can never see `Starting`. If the
    /// spawn fails, the session comes back to this thread and is stopped here,
    /// so no capture stream or thread outlives a start that did not happen;
    /// the caller's `fail_start` then removes the folder as for any other
    /// failed start.
    fn enter_recording(
        &self,
        app: &AppHandle,
        id: &str,
        started: chrono::DateTime<chrono::Local>,
        session: RecordingSession,
    ) -> Result<Status, UiError> {
        let on_fail = {
            let app = app.clone();
            move |session, message| {
                app.state::<Recorder>()
                    .fail_mid_recording(&app, session, message)
            }
        };

        let mut inner = self.lock();
        match Ticker::spawn(
            TICKER_THREAD_NAME,
            audio::session::TICK_INTERVAL,
            session,
            tick_session,
            on_fail,
        ) {
            Ok(ticker) => {
                inner.status.phase = Phase::Recording;
                inner.status.meeting_id = Some(id.to_string());
                inner.status.started_at_ms = Some(started.timestamp_millis());
                inner.ticker = Some(ticker);
                let status = inner.status.clone();
                drop(inner);
                if let Err(error) = app.emit(STATE_EVENT, &status) {
                    tracing::warn!(%error, "could not tell the window about a recording state change");
                }
                Ok(status)
            }
            Err((session, error)) => {
                drop(inner);
                tracing::error!(%error, "could not spawn the recording ticker thread");
                if let Err(message) = session.stop() {
                    tracing::warn!(%message, "the session behind a failed start did not stop cleanly");
                }
                Err(UiError::app(
                    "recorder-failed",
                    format!("could not start the recorder's checkpoint thread: {error}"),
                ))
            }
        }
    }

    /// Back to idle, never stuck in `Starting`, and never leaving a meeting
    /// folder with no audio in it behind — sitting in `Recording` with nothing
    /// written is the exact shape of the TUR-90 report; a folder from a start
    /// that never got that far is the same failure one step earlier.
    fn fail_start(&self, app: &AppHandle, id: Option<&str>, error: UiError) -> UiError {
        self.transition(app, |inner| *inner = Inner::idle());
        if let Some(id) = id {
            match crate::meetings::root() {
                Ok(root) => {
                    if let Err(remove_error) = std::fs::remove_dir_all(root.join(id)) {
                        tracing::warn!(
                            %remove_error,
                            meeting_id = id,
                            "could not remove the meeting folder left by a failed recording start"
                        );
                    }
                }
                Err(root_error) => tracing::warn!(
                    message = %root_error.message,
                    "could not resolve the meetings root to clean up a failed recording start"
                ),
            }
        }
        error
    }

    pub fn stop(&self, app: &AppHandle) -> Result<Status, UiError> {
        let ticker = match self.claim_stopping(app) {
            Err(status) => return Ok(status),
            Ok(ticker) => ticker,
        };

        let Some(ticker) = ticker else {
            tracing::error!("phase was Recording with no session attached; recovering to idle");
            return Ok(self.transition(app, |inner| *inner = Inner::idle()));
        };

        // TUR-97: end the ticker and join it *before* the session's own stop.
        // The session only comes back out of the join, so no tick can be
        // running, or start, once we hold it — a checkpoint can never race the
        // final header patch and `segments.json` write below. The wait is at
        // most one tick already in flight; the thread is woken, not polled.
        let session = match ticker.stop() {
            Ok(Some(session)) => session,
            Ok(None) => {
                // Only a failed tick's handler keeps the session, and it only
                // does so after claiming `Stopping` itself — which would have
                // made our claim above lose. Unreachable in practice; recover
                // rather than stick in `Stopping`.
                tracing::error!("the recording ticker kept the session; recovering to idle");
                return Ok(self.transition(app, |inner| *inner = Inner::idle()));
            }
            Err(message) => {
                tracing::error!(%message, "the recording ticker died; the session went with it");
                self.transition(app, |inner| *inner = Inner::idle());
                return Err(UiError::app("recorder-failed", message));
            }
        };

        match session.stop() {
            Ok(_report) => Ok(self.transition(app, |inner| *inner = Inner::idle())),
            Err(message) => {
                tracing::warn!(message = %message, "recording did not stop cleanly");
                self.transition(app, |inner| *inner = Inner::idle());
                Err(UiError::app("recorder-failed", message))
            }
        }
    }

    /// Claim `Recording -> Stopping` and take the session (inside its ticker)
    /// with it, or report the phase that beat us to it.
    fn claim_stopping(&self, app: &AppHandle) -> Result<Option<Ticker<RecordingSession>>, Status> {
        let mut inner = self.lock();
        if inner.status.phase != Phase::Recording {
            return Err(inner.status.clone());
        }
        inner.status.phase = Phase::Stopping;
        let ticker = inner.ticker.take();
        let status = inner.status.clone();
        drop(inner);
        if let Err(error) = app.emit(STATE_EVENT, &status) {
            tracing::warn!(%error, "could not tell the window about a recording state change");
        }
        Ok(ticker)
    }

    /// What a failed `tick()` means (TUR-97): the recording is over. Stop the
    /// session so what is on disk is finalised, go back to `Idle`, and tell
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
    fn fail_mid_recording(
        &self,
        app: &AppHandle,
        session: RecordingSession,
        message: String,
    ) -> Option<RecordingSession> {
        {
            let mut inner = self.lock();
            if inner.status.phase != Phase::Recording {
                tracing::error!(
                    %message,
                    "a recording tick failed while it was already being stopped"
                );
                return Some(session);
            }
            tracing::error!(%message, "a recording tick failed; stopping the recording");
            inner.status.phase = Phase::Stopping;
            if let Some(own) = inner.ticker.take() {
                own.detach();
            }
            let status = inner.status.clone();
            drop(inner);
            if let Err(error) = app.emit(STATE_EVENT, &status) {
                tracing::warn!(%error, "could not tell the window about a recording state change");
            }
        }

        // A panic in `stop()` must not strand the recorder in `Stopping`,
        // where every toggle is ignored: this thread is about to end, and
        // nobody else would ever move the phase on.
        let stopped =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || session.stop()))
                .unwrap_or_else(|payload| {
                    Err(format!(
                        "stopping panicked: {}",
                        ticker::panic_message(&*payload)
                    ))
                });
        let body = match &stopped {
            Ok(_report) => format!(
                "The recording stopped because of an error ({message}). The audio recorded up to \
                 that point was saved."
            ),
            Err(stop_message) => {
                tracing::warn!(message = %stop_message, "the failed recording did not stop cleanly");
                format!(
                    "The recording stopped because of an error ({message}), and finishing its \
                     files also failed ({stop_message})."
                )
            }
        };
        self.transition(app, |inner| *inner = Inner::idle());

        let error = UiError::app("recording-interrupted", body);
        if let Err(emit_error) = app.emit(ERROR_EVENT, &error) {
            tracing::warn!(%emit_error, "could not tell the window a recording failed");
        }
        notify_interrupted(app, &error.message);
        None
    }
}

/// One tick of the live session, with a panic turned into an ordinary tick
/// error (TUR-97). Without this, a panic would end the ticker thread silently:
/// the UI would keep showing `Recording` while nothing checkpointed — the
/// exact failure the ticker exists to remove — until the user pressed stop.
/// Routed through [`Recorder::fail_mid_recording`] instead, it stops the
/// session and tells the user like any other tick failure.
fn tick_session(session: &mut RecordingSession) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.tick())).unwrap_or_else(
        |payload| {
            Err(format!(
                "the recorder's checkpoint panicked: {}",
                ticker::panic_message(&*payload)
            ))
        },
    )
}

/// Tell the user a recording stopped on its own, on a surface that does not
/// need the window to be open — the same reasoning as `lib.rs`'s
/// `notify_refusal` for the shortcut.
fn notify_interrupted(app: &AppHandle, message: &str) {
    use tauri_plugin_notification::NotificationExt as _;

    if let Err(error) = app
        .notification()
        .builder()
        .title("meet-ai stopped recording")
        .body(message)
        .show()
    {
        tracing::warn!(%error, "could not show the interrupted-recording notification");
    }
}

/// Build a SPEC §3.1 meeting id: `YYYY-MM-DD-HHMM-slug`.
///
/// The slug is `meeting` until Phase 5a can name it from the calendar event.
/// A fixed slug is better than a guessed one — the list falls back to showing
/// the date and time, which is true, instead of a title nobody chose.
fn meeting_id(at: chrono::DateTime<chrono::Local>) -> String {
    format!("{}-meeting", at.format("%Y-%m-%d-%H%M"))
}

/// Create the meeting folder and the files SPEC §3.1 says live in it.
///
/// `transcript.md` is created empty and never written to here: §3.4 makes it
/// append-only and `crates/stt`'s `TranscriptSink` is the only thing allowed to
/// append. Creating it up front means the review view can open a meeting that
/// is still recording without a missing-file branch. `audio/` is also created
/// here, ahead of `RecordingSession::start`'s own (idempotent)
/// `create_dir_all`, so folder creation stays one step even though the audio
/// inside it is now the session's to write.
fn create_meeting_folder(id: &str) -> Result<(), UiError> {
    let dir = crate::meetings::root()?.join(id);
    std::fs::create_dir_all(dir.join("audio"))?;
    for file in ["transcript.md", "notes.md"] {
        let path = dir.join(file);
        if !path.exists() {
            std::fs::write(&path, "")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    #[test]
    fn meeting_ids_match_the_spec_3_1_folder_name() {
        let at = chrono::Local
            .with_ymd_and_hms(2026, 9, 1, 14, 30, 0)
            .single()
            .expect("a real local time");
        let id = meeting_id(at);
        assert_eq!(id, "2026-09-01-1430-meeting");

        // The list parser has to be able to read back what the recorder writes.
        // If these two ever disagree, every new recording shows up undated at
        // the bottom of the list and nobody notices until there are several.
        let (date, time, slug) = crate::meetings::split_folder_name(&id);
        assert_eq!(date.as_deref(), Some("2026-09-01"));
        assert_eq!(time.as_deref(), Some("14:30"));
        assert_eq!(slug.as_deref(), Some("meeting"));
    }

    #[test]
    fn a_fresh_recorder_is_idle() {
        let recorder = Recorder::default();
        let status = recorder.status();
        assert_eq!(status.phase, Phase::Idle);
        assert!(status.meeting_id.is_none());
        assert!(status.started_at_ms.is_none());
    }
}
