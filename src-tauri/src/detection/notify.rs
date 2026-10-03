//! The one notification path for every detection signal (TUR-27).
//!
//! Whatever noticed the meeting — a running app, audio activity, or the
//! calendar's one-minute reminder (TUR-30, [`remind`]) — ends up here. It says
//! *why* the user is being asked (from the [`Signal`]), posts a system
//! notification, and tells the window, whose `DetectionPrompt` banner holds
//! **Record** and **Dismiss** (and **Open brief** for a reminder). Two signals
//! for the same call make one prompt ([`super::merge`]).
//! macOS notifications posted through the plugin have no action buttons, so
//! the notification itself only explains; clicking it brings meet-ai forward,
//! where the banner is waiting. Nothing here starts a recording (L15).

use chrono::{DateTime, Utc};
use detect::Signal;
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Manager as _};

use super::Detection;
use super::merge::{Delivery, Merger};
use crate::events::DETECTION_PROMPT_EVENT;
use crate::recording::{Phase, Recorder};

/// The notification's title, for every signal.
pub const TITLE: &str = "Record this meeting?";

/// What the window hears on [`DETECTION_PROMPT_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    /// What was noticed.
    pub signal: Signal,
    /// Why the user is being asked, as one sentence: "Zoom is open."
    pub reason: String,
    /// Replace the prompt on screen, if there is one, and open none: this
    /// call was already asked about (`detection/merge.rs`).
    pub update_only: bool,
}

/// The prompt for `signal`, or `None` while a recording is starting, running
/// or stopping — the user is already recording, and asking again is nagging.
/// The detection loop already holds back while recording; this is the same
/// rule for every signal, at the one place they all pass.
pub fn prompt_for(phase: Phase, signal: &Signal) -> Option<Prompt> {
    (phase == Phase::Idle).then(|| Prompt {
        signal: signal.clone(),
        reason: signal.reason(),
        update_only: false,
    })
}

/// The notification's body: the reason, and where the buttons are.
fn body(prompt: &Prompt) -> String {
    match prompt.signal {
        Signal::Calendar { .. } => format!(
            "{} Open meet-ai to record it or read the brief.",
            prompt.reason
        ),
        Signal::Process { .. } | Signal::AudioActivity => {
            format!("{} Open meet-ai to record it.", prompt.reason)
        }
    }
}

/// Is a recording starting, running or stopping right now?
pub fn recording(app: &AppHandle) -> bool {
    current_phase(app) != Phase::Idle
}

fn current_phase(app: &AppHandle) -> Phase {
    app.try_state::<Recorder>()
        .map_or(Phase::Idle, |recorder| recorder.status().phase)
}

/// Ask the user whether to record, because of `signal` (a meeting app or
/// audio activity). Dropped if a reminder just asked about this call.
pub fn notify(app: &AppHandle, signal: &Signal) {
    deliver(app, signal, Merger::other);
}

/// Remind the user that `event` starts in a minute (TUR-30), and ask
/// whether to record it.
pub fn remind(app: &AppHandle, event: &::calendar::Event) {
    let signal = Signal::Calendar {
        title: event.title.clone(),
        attendees: event.attendees,
    };
    let ends = event.end;
    deliver(app, &signal, move |merger, now| merger.reminder(now, ends));
}

/// The one path every prompt takes: not while recording, merged with a
/// recent prompt for the same call, then a notification and the banner.
fn deliver(
    app: &AppHandle,
    signal: &Signal,
    merge: impl FnOnce(&mut Merger, DateTime<Utc>) -> Delivery,
) {
    use tauri_plugin_notification::NotificationExt as _;

    let Some(mut prompt) = prompt_for(current_phase(app), signal) else {
        tracing::debug!(?signal, "a meeting signal while recording; not asking");
        return;
    };
    let delivery = app
        .try_state::<Detection>()
        .map_or(Delivery::New, |detection| {
            detection.merge(|merger| merge(merger, Utc::now()))
        });
    match delivery {
        Delivery::Drop => {
            tracing::debug!(?signal, "already asked about this call; not asking again");
            return;
        }
        Delivery::UpdateOnly => prompt.update_only = true,
        Delivery::New => {}
    }
    tracing::info!(
        ?signal,
        ?delivery,
        "a meeting looks like it started; asking whether to record"
    );
    if delivery == Delivery::New
        && let Err(error) = app
            .notification()
            .builder()
            .title(TITLE)
            .body(body(&prompt))
            .show()
    {
        // The banner below still asks, whenever the window is looked at.
        tracing::warn!(%error, "could not show the detection notification");
    }
    if let Err(error) = app.emit(DETECTION_PROMPT_EVENT, &prompt) {
        tracing::warn!(%error, "could not send the detection prompt to the window");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zoom() -> Signal {
        Signal::Process {
            process: "zoom.us".to_string(),
        }
    }

    #[test]
    fn an_idle_recorder_is_asked_and_told_why() {
        let prompt = prompt_for(Phase::Idle, &zoom()).expect("asks when idle");
        assert_eq!(prompt.signal, zoom());
        assert_eq!(prompt.reason, "Zoom is open.");
        assert_eq!(body(&prompt), "Zoom is open. Open meet-ai to record it.");
    }

    #[test]
    fn no_prompt_while_recording() {
        for phase in [Phase::Starting, Phase::Recording, Phase::Stopping] {
            assert_eq!(prompt_for(phase, &zoom()), None, "{phase:?}");
            assert_eq!(prompt_for(phase, &Signal::AudioActivity), None, "{phase:?}");
        }
    }

    #[test]
    fn every_signal_takes_the_same_path() {
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        for signal in [zoom(), calendar, Signal::AudioActivity] {
            let prompt = prompt_for(Phase::Idle, &signal).expect("asks");
            assert_eq!(prompt.reason, signal.reason());
        }
    }

    #[test]
    fn the_window_gets_the_signal_and_the_reason() {
        let prompt = prompt_for(Phase::Idle, &zoom()).expect("asks");
        assert_eq!(
            serde_json::to_value(&prompt).expect("serialises"),
            serde_json::json!({
                "signal": { "kind": "process", "process": "zoom.us" },
                "reason": "Zoom is open.",
                "updateOnly": false
            })
        );
    }

    #[test]
    fn a_reminder_points_at_the_brief_too() {
        let calendar = Signal::Calendar {
            title: "Standup".to_string(),
            attendees: 3,
        };
        let prompt = prompt_for(Phase::Idle, &calendar).expect("asks");
        assert_eq!(
            body(&prompt),
            "“Standup” starts in a minute, with 3 people invited. \
             Open meet-ai to record it or read the brief."
        );
    }
}
