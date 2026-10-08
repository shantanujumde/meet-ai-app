//! Pause and resume (TUR-146): the recorder's half.
//!
//! The session the ticker thread owns does the audio half
//! (`audio::session::PauseSwitch`): at its next tick it stops both channels,
//! so nothing is written or transcribed, and on resume it starts them again
//! into the same files. This half keeps the phase `Recording` throughout (a
//! paused recording is still one meeting, and Stop still ends it), flips the
//! switch, and keeps the [`PauseClock`] on the status so every window's timer
//! leaves the paused stretches out.

use serde::Serialize;
use tauri::AppHandle;

use super::{Inner, Phase, Recorder, Status, emit_state};

/// The paused stretches of the recording, on [`Status::pause`]. The window
/// works the timer out from these and `started_at_ms`, as it already does,
/// rather than being sent a tick a second: the elapsed time is the time
/// since the start, less `paused_total_ms`, frozen at `paused_at_ms` while a
/// pause runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PauseClock {
    /// Unix epoch milliseconds the current pause began; `None` while recording.
    #[specta(type = Option<specta_typescript::Number>)]
    pub paused_at_ms: Option<i64>,
    /// Milliseconds of every earlier, finished pause, summed.
    #[specta(type = specta_typescript::Number)]
    pub paused_total_ms: i64,
}

impl PauseClock {
    /// Whether a pause is running.
    pub fn is_paused(&self) -> bool {
        self.paused_at_ms.is_some()
    }

    /// Start a pause at `now_ms`. `false` when one is already running.
    fn pause(&mut self, now_ms: i64) -> bool {
        if self.is_paused() {
            return false;
        }
        self.paused_at_ms = Some(now_ms);
        true
    }

    /// End the pause at `now_ms`, adding its length to the total. `false`
    /// when none is running. A clock that went backwards adds nothing.
    fn resume(&mut self, now_ms: i64) -> bool {
        let Some(at) = self.paused_at_ms.take() else {
            return false;
        };
        self.paused_total_ms += now_ms.saturating_sub(at).max(0);
        true
    }
}

impl Inner {
    /// Pause (`paused`) or resume the live recording at `now_ms`. `None`
    /// when nothing changed: not recording, or already that way.
    fn set_paused(&mut self, paused: bool, now_ms: i64) -> Option<Status> {
        if self.status.phase != Phase::Recording {
            return None;
        }
        let switch = self.pause.as_ref()?;
        let changed = if paused {
            self.status.pause.pause(now_ms)
        } else {
            self.status.pause.resume(now_ms)
        };
        if !changed {
            return None;
        }
        switch.set(paused);
        Some(self.status.clone())
    }
}

impl Recorder {
    /// Pause the live recording. Nothing is written or transcribed until
    /// [`Recorder::resume`]; anything but a live recording is left alone.
    pub fn pause(&self, app: &AppHandle) -> Status {
        self.set_paused(app, true)
    }

    /// Carry on recording into the same meeting after [`Recorder::pause`].
    pub fn resume(&self, app: &AppHandle) -> Status {
        self.set_paused(app, false)
    }

    fn set_paused(&self, app: &AppHandle, paused: bool) -> Status {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let changed = self.lock().set_paused(paused, now_ms);
        match changed {
            Some(status) => {
                tracing::info!(paused, "recording paused or resumed");
                emit_state(app, &status);
                status
            }
            None => self.status(),
        }
    }
}

#[cfg(test)]
mod tests {
    use audio::session::PauseSwitch;

    use super::*;

    fn recording() -> (Inner, PauseSwitch) {
        let mut inner = Inner::idle();
        inner.enter_starting().expect("a fresh recorder is idle");
        inner.status.phase = Phase::Recording;
        inner.status.started_at_ms = Some(1_000);
        let switch = PauseSwitch::default();
        inner.pause = Some(switch.clone());
        (inner, switch)
    }

    #[test]
    fn a_pause_flips_the_session_switch_and_freezes_the_clock() {
        let (mut inner, switch) = recording();
        let paused = inner.set_paused(true, 5_000).expect("a live recording pauses");
        assert_eq!(paused.phase, Phase::Recording, "still one meeting");
        assert_eq!(paused.pause.paused_at_ms, Some(5_000));
        assert!(switch.wanted(), "the session is asked to stop both channels");

        assert!(inner.set_paused(true, 6_000).is_none(), "already paused");
        assert_eq!(inner.status.pause.paused_at_ms, Some(5_000));

        let resumed = inner.set_paused(false, 9_000).expect("and resumes");
        assert!(!resumed.pause.is_paused());
        assert_eq!(resumed.pause.paused_total_ms, 4_000);
        assert!(!switch.wanted());
        assert!(inner.set_paused(false, 9_500).is_none(), "already recording");
    }

    #[test]
    fn every_pause_adds_to_the_total() {
        let mut clock = PauseClock::default();
        assert!(clock.pause(10) && clock.resume(30));
        assert!(clock.pause(100) && clock.resume(150));
        assert_eq!(clock.paused_total_ms, 70);
        assert!(!clock.resume(200), "no pause running");
        assert!(clock.pause(300) && clock.resume(250), "the clock went back");
        assert_eq!(clock.paused_total_ms, 70, "a backwards clock adds nothing");
    }

    #[test]
    fn only_a_live_recording_pauses() {
        let mut inner = Inner::idle();
        assert!(inner.set_paused(true, 1).is_none(), "idle");
        let (mut inner, switch) = recording();
        inner.status.phase = Phase::Stopping;
        assert!(inner.set_paused(true, 1).is_none(), "stopping");
        assert!(!switch.wanted());
    }

    #[test]
    fn a_stop_and_the_next_start_clear_the_pause() {
        let (mut inner, _switch) = recording();
        inner.set_paused(true, 2_000).unwrap();
        inner.end(None);
        assert_eq!(inner.status.pause, PauseClock::default());
        assert!(inner.pause.is_none(), "the switch goes with the session");
        let starting = inner.enter_starting().unwrap();
        assert_eq!(starting.pause, PauseClock::default());
    }

    #[test]
    fn the_clock_crosses_the_wire_in_camel_case() {
        let clock = PauseClock {
            paused_at_ms: Some(42),
            paused_total_ms: 7,
        };
        assert_eq!(
            serde_json::to_value(clock).unwrap(),
            serde_json::json!({ "pausedAtMs": 42, "pausedTotalMs": 7 })
        );
        let json = serde_json::to_value(Status::idle()).unwrap();
        assert_eq!(
            json["pause"],
            serde_json::json!({ "pausedAtMs": null, "pausedTotalMs": 0 })
        );
    }
}
