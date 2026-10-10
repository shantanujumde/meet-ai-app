//! The recording lifecycle: start, stop, and what ⌘⇧R does.
//!
//! The recorder behind this is real (TUR-94's `audio::session::RecordingSession`):
//! start opens the microphone and, where available, the system-audio tap and
//! writes into the SPEC §3.1 meeting folder this module creates; stop closes
//! both cleanly and leaves `mic.wav`, `system.wav` and `segments.json` complete,
//! with no later repair step needed.
//!
//! Starting and stopping both take real wall-clock time (Core Audio warming
//! up each channel; the permission chime runs during the recording, TUR-136)
//! so every entry point here is meant to be called off a thread that
//! must stay responsive. `commands::toggle_recording`/`stop_recording` run this
//! on a blocking thread the same way `commands::measure_permission` does; the
//! ⌘⇧R handler in `lib.rs` gives it a worker thread of its own so the shortcut
//! callback never blocks. Nothing here needs to know which caller it is: the
//! phase check and transition happen inside one `Mutex`, so whichever caller's
//! lock lands first wins a race and the other sees `Starting`/`Stopping` and
//! no-ops, rather than racing a second tap open.
//!
//! Live transcription (TUR-96) rides alongside, never in front: the session is
//! started with a tee per channel, and [`crate::live_transcript`] turns those
//! into lines. It starts only once the audio is already flowing, it cannot fail
//! a start or a stop, and on stop it is finished *after* the audio is safely
//! closed — so the worst an engine can do is leave `transcript.md` short, with
//! the WAVs still whole. Every ending goes through [`stop::close`] (TUR-161).
//!
//! While a recording is live, a ticker thread ([`ticker`], TUR-97) owns the
//! `RecordingSession` and calls its `tick()` every
//! [`audio::session::TICK_INTERVAL`], which is what checkpoints the WAV headers
//! and `segments.json` every five seconds and reopens a segment on a default
//! device change. The `Mutex` holds that ticker instead of the bare session, so
//! the phase and the thing it owns still change together under one lock, and
//! the lock is never held across a tick.
//!
//! Whatever ends a recording or refuses a start without a clean answer — a
//! tick that failed, a stop that could not close the files, a start that was
//! refused — is said once, on the idle status [`RECORDING_STATE_EVENT`] carries
//! ([`Status::error`]). There is no second error event: the window reads the
//! reason off the same status that moves it to `Idle`, so the two can never
//! arrive apart or out of order, and the interrupted-recording notification
//! is built from that status too.

pub(crate) mod auto_title;
pub(crate) mod backup_stop;
mod interrupted;
// TUR-146: pause and resume.
pub(crate) mod pause;
mod phase;
mod settle; // TUR-160: quitting waits for a start or stop to finish.
mod start_check;
// TUR-161: every way a recording ends, through one helper.
mod stop;
mod ticker;

use std::sync::Arc;

use audio::AudioSource;
use audio::session::{PauseSwitch, RecordingSession};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};
use {::calendar::Event, meeting_format::layout, stt::Speaker};

pub use self::phase::Phase;
use self::ticker::Ticker;
use store::folder_name::create_meeting_folder;

use crate::error::UiError;
use crate::events::RECORDING_STATE_EVENT;
use crate::live_transcript::{self, LiveTranscript, Transcription};

/// The ticker thread's name, so it is identifiable in a sample or a crash
/// report next to `meet-ai-record-shortcut`.
const TICKER_THREAD_NAME: &str = "meet-ai-recording-ticker";

/// The recorder's state as the webview sees it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    /// The folder name of the meeting being recorded, `None` when idle.
    pub meeting_id: Option<String>,
    /// Unix epoch milliseconds the recording started, so the UI can run its own
    /// timer instead of being fed a tick per second over IPC.
    #[specta(type = Option<specta_typescript::Number>)]
    pub started_at_ms: Option<i64>,
    /// Why the last recording ended badly, or the last start was refused: it
    /// stopped on its own because a tick failed (TUR-97), it did not close
    /// cleanly, or a start from ⌘⇧R, the menu bar or the button was turned
    /// down (TUR-127). Only ever set on an `Idle` status, and cleared by the
    /// next start. It rides on the state event because most of these have no
    /// command call waiting to return an error to — a recording that ends on
    /// its own, or a shortcut pressed with the window hidden — and one event
    /// for the move to `Idle` and its reason means the window can never show
    /// one without the other. Serialised as `null` when nothing went wrong.
    pub error: Option<UiError>,
    /// The paused stretches, so the timer leaves them out (TUR-146). Boxed: keeps `Status` small.
    pub pause: Box<pause::PauseClock>,
}

impl Status {
    fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            meeting_id: None,
            started_at_ms: None,
            error: None,
            pause: Box::default(),
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
    /// The live transcript riding on the session's tees (TUR-96). Taken
    /// together with the ticker on stop, so the two always end as a pair.
    transcription: Option<Transcription>,
    /// How the window pauses the session the ticker owns (TUR-146).
    pause: Option<PauseSwitch>,
}

impl Inner {
    fn idle() -> Self {
        Self {
            status: Status::idle(),
            ticker: None,
            transcription: None,
            pause: None,
        }
    }

    /// Back to idle, saying why when the recording (or the start) did not end
    /// cleanly. `Some` only for an ending the user should hear about; a clean
    /// stop passes `None`.
    fn end(&mut self, error: Option<UiError>) {
        *self = Inner::idle();
        self.status.error = error;
    }

    /// `Idle -> Starting`, or the phase that beat us to it. A new start is
    /// what clears the last recording's error: from here on the status
    /// describes this attempt, and a refusal puts its own reason back.
    fn enter_starting(&mut self) -> Result<Status, Status> {
        if self.status.phase != Phase::Idle {
            return Err(self.status.clone());
        }
        self.status.phase = Phase::Starting;
        self.status.error = None;
        Ok(self.status.clone())
    }

    /// A start turned away before it reached [`Recorder::start`] — the
    /// meetings folder is moving, so `folder_move::FolderGate` never let the
    /// toggle run. Only an idle recorder takes the reason: a move can only
    /// begin while idle, so that is the one phase such a refusal can meet,
    /// and a status mid-recording must never carry an error. `None` when
    /// nothing changed.
    fn refuse_start(&mut self, error: UiError) -> Option<Status> {
        if self.status.phase != Phase::Idle {
            return None;
        }
        self.status.error = Some(error);
        Some(self.status.clone())
    }
}

/// Managed Tauri state. One recorder per app, because two would fight over the
/// system audio tap — the same reason the single-instance plugin is wired up.
pub struct Recorder {
    inner: settle::Watched<Inner>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            inner: settle::Watched::new(Inner::idle()),
        }
    }
}

impl Recorder {
    pub fn status(&self) -> Status {
        self.lock().status.clone()
    }

    /// Put a refusal from outside the recorder on the idle status, the same
    /// place [`Recorder::fail_start`] puts one from inside it, so the window
    /// hears about a refused ⌘⇧R or menu-bar press whichever layer said no —
    /// there is no separate error event to carry it any more.
    pub fn refuse_start(&self, app: &AppHandle, error: &UiError) {
        let refused = self.lock().refuse_start(error.clone());
        if let Some(status) = refused {
            emit_state(app, &status);
        }
    }

    /// A poisoned lock here means a previous call panicked while holding it.
    /// Recovering is strictly better than taking the whole app down; the worst
    /// case is a stuck phase, not a half-broken invariant.
    fn lock(&self) -> settle::Guard<'_, Inner> {
        self.inner.lock()
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
        emit_state(app, &next);
        next
    }

    /// Start if idle, stop if recording, and do nothing mid-transition.
    ///
    /// One entry point for the button, the menu item and the global shortcut,
    /// so all three cannot disagree about what "toggle" means.
    pub fn toggle(&self, app: &AppHandle) -> Result<Status, UiError> {
        match self.status().phase {
            Phase::Idle => self.start(app, None),
            Phase::Recording => self.stop(app),
            // Mid-transition. Returning the current state rather than an error
            // is correct: the user pressed a toggle and the answer is "already
            // doing that", which is not a failure worth interrupting them for.
            Phase::Starting | Phase::Stopping => Ok(self.status()),
        }
    }

    /// Claim `Idle -> Starting`, or report the phase that beat us to it.
    fn claim_starting(&self, app: &AppHandle) -> Result<(), Status> {
        let status = self.lock().enter_starting()?;
        emit_state(app, &status);
        Ok(())
    }

    pub fn start(&self, app: &AppHandle, pinned: Option<Event>) -> Result<Status, UiError> {
        if let Err(status) = self.claim_starting(app) {
            return Ok(status);
        }

        // TUR-136: only the silent microphone decision runs before the start.
        // SPEC §8.1's positive control for system audio (the chime) runs
        // during the recording, `start_check`, and never stops it. This is
        // still the backstop for ⌘⇧R, which has no button to disable.
        if let Err(error) = start_check::before_start(app) {
            return Err(self.fail_start(app, None, error));
        }

        let started = chrono::Local::now();
        // TUR-97: each candidate id is published before its folder exists, so
        // a meetings list taken mid-start (`meetings::Live`) always knows which
        // folder is live and never mistakes it for an interrupted one.
        // `fail_start` clears it with the rest of `Inner`. `None` on failure:
        // the folder that failed may be an earlier meeting's, and cleaning up
        // must never delete that.
        let id = match crate::meetings::root().and_then(|root| {
            Ok(create_meeting_folder(
                &root,
                &meeting_id(started),
                |candidate| {
                    self.lock().status.meeting_id = Some(candidate.to_string());
                },
            )?)
        }) {
            Ok(id) => id,
            Err(error) => return Err(self.fail_start(app, None, error)),
        };

        let meeting_dir = match crate::meetings::root() {
            Ok(root) => root.join(&id),
            Err(error) => return Err(self.fail_start(app, Some(&id), error)),
        };

        let mic: Box<dyn AudioSource> = Box::new(audio::mic::MicSource::new());
        let sys = audio::session::default_system_source();

        // One tee per channel (TUR-31), handed out before the speech engine
        // is known to start: an unread tee costs capture nothing. They also feed
        // the permission check (TUR-136) and the silence stop (TUR-145).
        let (tees, mic_feed, sys_feed, check) = backup_stop::tees();

        match RecordingSession::start_with_tees(layout::audio_dir(&meeting_dir), mic, sys, tees) {
            Ok(session) => {
                let check = check.attach(&session);
                // Mic is `You`, system audio is `Others` (L5). A requested
                // system track keeps its session even if it is down now: the
                // next device change rebuilds the tap (TUR-121).
                let mut tracks = vec![(Speaker::You, mic_feed)];
                if session.wants_system_audio() {
                    tracks.push((Speaker::Others, sys_feed));
                }
                let transcription = app.state::<LiveTranscript>().start(
                    Arc::new(app.clone()),
                    layout::transcript_path(&meeting_dir),
                    tracks,
                    live_transcript::Plan::configured(),
                );
                self.enter_recording(app, &id, started, session, transcription)
                    .inspect(|_| auto_title::spawn(app, &id, started, pinned))
                    .inspect(|_| check.spawn(app))
                    .map_err(|error| self.fail_start(app, Some(&id), error))
            }
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
        transcription: Transcription,
    ) -> Result<Status, UiError> {
        let on_fail = {
            let app = app.clone();
            move |session, message| {
                app.state::<Recorder>()
                    .fail_mid_recording(&app, session, message)
            }
        };

        let pause = session.pause_switch();
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
                inner.transcription = Some(transcription);
                inner.pause = Some(pause);
                let status = inner.status.clone();
                drop(inner);
                emit_state(app, &status);
                Ok(status)
            }
            Err((session, error)) => {
                drop(inner);
                tracing::error!(%error, "could not spawn the recording ticker thread");
                let (session, transcription) = (Some(session), Some(transcription));
                let ending = stop::Ending::NeverStarted;
                Err(stop::close(
                    &stop::Live(app),
                    session,
                    transcription,
                    ending,
                    |stopped| {
                        if let Some(Err(message)) = stopped {
                            tracing::warn!(%message, "the session behind a failed start did not stop cleanly");
                        }
                        UiError::app(
                            "recorder-failed",
                            format!("could not start the recorder's checkpoint thread: {error}"),
                        )
                    },
                ))
            }
        }
    }

    /// Back to idle, never stuck in `Starting`, and never leaving a meeting
    /// folder with no audio in it behind — sitting in `Recording` with nothing
    /// written is the exact shape of the TUR-90 report; a folder from a start
    /// that never got that far is the same failure one step earlier.
    ///
    /// The idle status carries the refusal too (TUR-127): a start from ⌘⇧R or
    /// the menu bar has no command call to return it to, and the window still
    /// has to say why nothing happened.
    fn fail_start(&self, app: &AppHandle, id: Option<&str>, error: UiError) -> UiError {
        self.transition(app, |inner| inner.end(Some(error.clone())));
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
}

/// Tell every window about a state change. A webview that has gone away is
/// not an error worth propagating up into a recording control.
fn emit_state(app: &AppHandle, status: &Status) {
    if let Err(error) = app.emit(RECORDING_STATE_EVENT, status) {
        tracing::warn!(%error, "could not tell the window about a recording state change");
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
                crate::worker::panic_message(&*payload)
            ))
        },
    )
}

/// Build a SPEC §3.1 meeting id: `YYYY-MM-DD-HHMM-meeting`. The slug never
/// changes; the calendar names the meeting in `meeting.md` ([`auto_title`]).
fn meeting_id(at: chrono::DateTime<chrono::Local>) -> String {
    store::folder_name::meeting_id(&at.format("%Y-%m-%d-%H%M").to_string())
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
    fn a_second_recording_in_the_same_minute_gets_its_own_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let base = "2026-09-01-1430-meeting";

        let first = create_meeting_folder(root, base, |_| {}).unwrap();
        assert_eq!(first, base);
        // The first meeting has content by the time the second starts.
        std::fs::write(
            root.join(&first).join("transcript.md"),
            "[00:00:04] You: Hi.\n",
        )
        .unwrap();

        let second = create_meeting_folder(root, base, |_| {}).unwrap();
        let third = create_meeting_folder(root, base, |_| {}).unwrap();
        assert_eq!(second, "2026-09-01-1430-meeting-2");
        assert_eq!(third, "2026-09-01-1430-meeting-3");

        assert_eq!(
            std::fs::read_to_string(root.join(&first).join("transcript.md")).unwrap(),
            "[00:00:04] You: Hi.\n",
            "the first meeting's transcript is untouched"
        );
        for id in [&second, &third] {
            let dir = root.join(id);
            assert!(dir.join("audio").is_dir());
            assert_eq!(
                std::fs::read_to_string(dir.join("transcript.md")).unwrap(),
                ""
            );
            // Still a name the meeting list and TUR-99's store can read.
            let (date, time, slug) = crate::meetings::split_folder_name(id);
            assert_eq!(date.as_deref(), Some("2026-09-01"));
            assert_eq!(time.as_deref(), Some("14:30"));
            assert!(slug.unwrap().starts_with("meeting-"));
        }
    }

    #[test]
    fn a_fresh_recorder_is_idle() {
        let recorder = Recorder::default();
        let status = recorder.status();
        assert_eq!(status.phase, Phase::Idle);
        assert!(status.meeting_id.is_none());
        assert!(status.started_at_ms.is_none());
        assert!(status.error.is_none());
    }

    /// The window reads `error` off every state event: `null` when nothing
    /// went wrong, a `UiError` in the same camelCase shape a rejected command
    /// gives otherwise, so one banner shows both.
    #[test]
    fn the_status_error_crosses_the_wire_as_null_or_a_ui_error() {
        let mut status = Status::idle();
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["error"], serde_json::Value::Null);
        assert_eq!(json["startedAtMs"], serde_json::Value::Null);

        status.error = Some(UiError::app("recording-interrupted", "disk full"));
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["error"]["domain"], "app");
        assert_eq!(json["error"]["kind"], "recording-interrupted");
        assert_eq!(json["error"]["message"], "disk full");
    }

    /// A start the folder gate turned away lands on the idle status like one
    /// the recorder refused itself, and never on a live recording's status.
    #[test]
    fn a_start_refused_before_the_recorder_still_leaves_its_reason() {
        let mut inner = Inner::idle();
        let refused = UiError::app("folder-move-in-progress", "moving");
        let status = inner.refuse_start(refused).expect("idle takes it");
        assert_eq!(status.phase, Phase::Idle);
        assert_eq!(status.error.unwrap().kind, "folder-move-in-progress");
        // The next start clears it, as for any other reason.
        assert!(inner.enter_starting().unwrap().error.is_none());

        inner.status.phase = Phase::Recording;
        assert!(
            inner
                .refuse_start(UiError::app("folder-move-in-progress", "moving"))
                .is_none()
        );
        assert!(inner.status.error.is_none());
    }

    /// A clean stop leaves no error behind, and a start that is refused puts
    /// its own reason on the idle status (TUR-127: ⌘⇧R has nowhere else to
    /// say it).
    #[test]
    fn only_an_unclean_ending_leaves_an_error_on_the_idle_status() {
        let mut inner = Inner::idle();
        inner.enter_starting().unwrap();
        inner.status.phase = Phase::Recording;
        inner.end(None);
        assert_eq!(inner.status.phase, Phase::Idle);
        assert!(inner.status.error.is_none());

        inner.enter_starting().unwrap();
        inner.end(Some(UiError::app("permission-denied", "not allowed")));
        assert_eq!(inner.status.phase, Phase::Idle);
        assert_eq!(
            inner.status.error.as_ref().unwrap().kind,
            "permission-denied"
        );

        // Mid-transition, a second start is turned away without touching it.
        inner.status.phase = Phase::Stopping;
        assert!(inner.enter_starting().is_err());
        assert!(inner.status.error.is_some());
    }
}
