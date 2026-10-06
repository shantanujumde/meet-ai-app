//! The audio-permission checks around a recording start (TUR-136).
//!
//! Record used to wait about 10 s before anything was recorded: SPEC §8.1's
//! positive control (the chime, played and listened for on a tap of its own,
//! A7) ran first. Now only the silent, instant half runs before the start,
//! [`before_start`]: a stored microphone denial is the one answer that
//! refuses a recording, as before. The system-audio check runs **during** the
//! recording, on its own thread, reading a second copy of the recording's
//! own system stream ([`audio::tee::Tee::fan_out`]), so the live transcript's
//! copy is untouched. Its chime is hushed out of the live transcript
//! ([`audio::tee::Hush`]), and its verdict can only ever do two things:
//!
//! - tell the window ([`PERMISSION_STATUS_EVENT`]), which shows "System audio
//!   is off" when it is;
//! - when system audio is denied, ask the session to drop its system track
//!   ([`SystemDrop`]): the segment closes with `system_audio_denied` and the
//!   recording goes on with the microphone only.
//!
//! It never stops a recording. A check cut short (the recording stopped
//! first, the tap stalled) is unmeasurable and changes nothing.

use audio::permission_check::{ChannelResult, during_recording};
use audio::session::{RecordingSession, SystemDrop, Tees};
use audio::tee::{Hush, TeeFeed};
use meeting_format::segments::reason;
use tauri::{AppHandle, Emitter as _};

use crate::error::UiError;
use crate::events::PERMISSION_STATUS_EVENT;
use crate::permission::{self, Pane, Status};

/// The check thread's name, next to `meet-ai-recording-ticker` in a sample.
const CHECK_THREAD_NAME: &str = "meet-ai-system-audio-check";

/// The check before a recording starts: instant and silent. `Err` is the
/// refusal when the microphone is denied; nothing else refuses.
pub(super) fn before_start(app: &AppHandle) -> Result<(), UiError> {
    // TUR-91: the microphone the recording opens follows the setting as
    // `config.jsonc` has it now.
    crate::mic_setting::apply();
    let permission = permission::quick();
    tracing::info!(
        state = ?permission.state,
        detail = %permission.detail,
        "permission check before recording"
    );
    match refusal(&permission) {
        Some(error) => {
            // The window mirrors this answer (Record disabled, "Fix this").
            emit(app, &permission);
            Err(error)
        }
        None => Ok(()),
    }
}

/// Why `permission` refuses a recording, if it does.
fn refusal(permission: &Status) -> Option<UiError> {
    permission
        .blocks_recording()
        .then(|| UiError::app("permission-denied", permission.refusal_message()))
}

/// The check's half of a recording's tees, until the session exists.
pub(super) struct Check {
    feed: TeeFeed,
    hush: Hush,
    drop: SystemDrop,
}

/// The tees a recording starts with and the feeds they lead to: one per
/// channel for the live transcript, both hushed while the check's chime
/// plays, and the system one fanned out to the check.
pub(super) fn tees() -> (Tees, TeeFeed, TeeFeed, Check) {
    let hush = Hush::default();
    let (mic_tee, mic_feed) = audio::tee::tee();
    let (sys_tee, sys_feed) = audio::tee::tee();
    let (check_tee, check_feed) = audio::tee::tee();
    let tees = Tees {
        mic: Some(mic_tee.with_hush(hush.clone())),
        sys: Some(sys_tee.with_hush(hush.clone()).fan_out(check_tee)),
    };
    let check = Check {
        feed: check_feed,
        hush,
        drop: SystemDrop::default(),
    };
    (tees, mic_feed, sys_feed, check)
}

impl Check {
    /// Point the check at the session it may drop the system track of.
    pub(super) fn attach(self, session: &RecordingSession) -> Self {
        Self {
            drop: session.system_drop(),
            ..self
        }
    }

    /// Start the check on its own thread and return at once. A thread that
    /// cannot be spawned is logged, and the recording goes on unchecked.
    pub(super) fn spawn(self, app: &AppHandle) {
        let app = app.clone();
        let spawned = spawn_with(
            self,
            |feed, hush| during_recording::check(feed, &hush),
            move |status| emit(&app, status),
        );
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the system-audio check; recording unchecked");
        }
    }
}

/// [`Check::spawn`] with the check and the report passed in, so the thread,
/// the verdict and the drop are tested without an `AppHandle` or audio.
fn spawn_with(
    check: Check,
    run: impl FnOnce(TeeFeed, Hush) -> ChannelResult + Send + 'static,
    report: impl FnOnce(&Status) + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name(CHECK_THREAD_NAME.to_string())
        .spawn(move || {
            let Check { feed, hush, drop } = check;
            let status = permission::during_recording(run(feed, hush));
            tracing::info!(
                state = ?status.state,
                detail = %status.detail,
                "system-audio check during recording"
            );
            act_on(&status, &drop);
            report(&status);
        })
}

/// The one thing a verdict can do to the recording: drop the system track
/// when system audio is denied. Returns whether it asked.
fn act_on(status: &Status, drop: &SystemDrop) -> bool {
    let denied = status.denied.contains(&Pane::AudioCapture);
    if denied {
        tracing::warn!("system audio is denied; the recording goes on with the microphone only");
        drop.request(reason::SYSTEM_AUDIO_DENIED);
    }
    denied
}

/// Tell the window. A webview that has gone away is not worth an error.
fn emit(app: &AppHandle, status: &Status) {
    if let Err(error) = app.emit(PERMISSION_STATUS_EVENT, status) {
        tracing::warn!(%error, "could not tell the window about the permission check");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use audio::permission_check::ChannelState;

    use super::*;
    use crate::permission::State;

    fn verdict(state: ChannelState) -> ChannelResult {
        ChannelResult {
            state,
            detail: "fake check".into(),
        }
    }

    fn check() -> (Check, SystemDrop) {
        let (_tees, _mic, _sys, check) = tees();
        let drop = check.drop.clone();
        (check, drop)
    }

    /// The whole point of TUR-136: a check as slow as the old one (10 s)
    /// no longer holds the start. `spawn_with` is what `Recorder::start`
    /// calls once the recording runs, and it returns at once.
    #[test]
    fn a_check_that_takes_ten_seconds_does_not_hold_the_start() {
        let (check, _drop) = check();
        let (release, released) = mpsc::channel::<()>();
        let (done, finished) = mpsc::channel::<Status>();
        let started = Instant::now();
        let thread = spawn_with(
            check,
            move |_feed, _hush| {
                // A fake check that takes 10 s unless the test lets it go.
                let _ = released.recv_timeout(Duration::from_secs(10));
                verdict(ChannelState::Granted)
            },
            move |status| done.send(status.clone()).unwrap(),
        )
        .expect("the check thread spawns");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "start waited {:?} for the check",
            started.elapsed()
        );
        assert!(
            finished.try_recv().is_err(),
            "the check is still running after the start returned"
        );
        release.send(()).unwrap();
        thread.join().unwrap();
        assert_eq!(finished.recv().unwrap().state, State::Granted);
    }

    #[test]
    fn a_denied_verdict_drops_the_system_track_and_never_refuses() {
        let (check, drop) = check();
        let (done, finished) = mpsc::channel::<Status>();
        spawn_with(
            check,
            |_feed, _hush| verdict(ChannelState::Denied),
            move |status| done.send(status.clone()).unwrap(),
        )
        .unwrap()
        .join()
        .unwrap();
        assert_eq!(drop.take().as_deref(), Some(reason::SYSTEM_AUDIO_DENIED));

        let status = finished.recv().unwrap();
        assert!(
            status.system_audio_off(),
            "the window says system audio is off"
        );
        assert!(
            !status.blocks_recording(),
            "the microphone is recording, so nothing refuses"
        );
    }

    #[test]
    fn any_other_verdict_leaves_the_system_track_alone() {
        for (state, expected) in [
            (ChannelState::Granted, State::Granted),
            (ChannelState::NotApplicable, State::Granted),
            // Cut short or no output device: proves nothing (TUR-72).
            (ChannelState::Unmeasurable, State::Unknown),
        ] {
            let status = permission::during_recording(verdict(state.clone()));
            let drop = SystemDrop::default();
            assert!(!act_on(&status, &drop), "{state:?}");
            assert_eq!(drop.take(), None, "{state:?}");
            assert_eq!(status.state, expected, "{state:?}");
        }
    }

    #[test]
    fn only_a_denied_microphone_refuses_before_the_start() {
        let unknown = Status {
            state: State::Unknown,
            measured: false,
            detail: "not checked".into(),
            denied: Vec::new(),
        };
        assert!(refusal(&unknown).is_none(), "unknown starts at once");

        let mic_denied = Status {
            state: State::Denied,
            measured: true,
            detail: "microphone off".into(),
            denied: vec![Pane::Microphone],
        };
        let error = refusal(&mic_denied).expect("a denied microphone refuses");
        assert_eq!(error.kind, "permission-denied");
    }

    #[test]
    fn the_transcript_tees_are_hushed_and_the_check_gets_its_own_copy() {
        let (tees, mic_feed, sys_feed, check) = tees();
        let (mic, sys) = (tees.mic.unwrap(), tees.sys.unwrap());
        check.hush.hold_for(Duration::from_secs(3600));
        mic.offer(&[9, 9]);
        sys.offer(&[7, 7, 7]);
        assert_eq!(mic_feed.try_recv().unwrap(), [0, 0], "chime hushed");
        assert_eq!(sys_feed.try_recv().unwrap(), [0, 0, 0], "chime hushed");
        assert_eq!(
            check.feed.try_recv().unwrap(),
            [7, 7, 7],
            "the check hears it"
        );
    }
}
