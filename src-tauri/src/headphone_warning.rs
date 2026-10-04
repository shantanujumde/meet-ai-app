//! The "No headphones" warning while recording (TUR-65, SPEC L6).
//!
//! Without headphones the mic hears the other people through the speakers,
//! so both tracks hold the same voices and the transcript repeats lines.
//! SPEC L6 says warn, never fix in code. When a recording starts, a thread
//! reads the default output every [`POLL_INTERVAL`] until it stops: plugging
//! headphones into the jack does not change the default device on every OS
//! (the built-in output only switches its data source or port), so the
//! whole reading is polled, not just the device id. A change between
//! "warn" and "don't" is sent as [`HEADPHONE_WARNING_EVENT`]; the window
//! asks [`headphone_warning`] for the current one when it opens.
//!
//! Follows the recorder's own state event, like `hooks::app`, so the
//! recorder needs no change. The reading never blocks or delays recording,
//! and an output that cannot be told apart (`OutputKind::Unknown`) never
//! warns. `audio.warn_no_headphones` off: no reads at all.

use std::sync::Mutex;
use std::time::Duration;

use audio::headphones::{OutputKind, default_output_kind, should_warn};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, Listener as _};

use crate::events::{HEADPHONE_WARNING_EVENT, RECORDING_STATE_EVENT};
use crate::lock::lock_or_recover;

/// How often the output is read while recording. A few seconds of a missing
/// banner after plugging speakers in costs nothing; the read is cheap
/// property calls.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// What [`HEADPHONE_WARNING_EVENT`] carries, and [`headphone_warning`]
/// returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HeadphoneWarning {
    /// The meeting being recorded.
    pub meeting_id: String,
    /// Show the banner: the output is speakers and the setting is on.
    pub show: bool,
}

/// The recording being watched and what was last sent for it.
#[derive(Debug, Default)]
pub(crate) struct Tracker {
    meeting_id: Option<String>,
    shown: Option<bool>,
}

/// What a recorder status means for the watch.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// A recording started: start reading the output for it.
    Started(String),
    /// The recording stopped; send this if the banner was up.
    Stopped(Option<HeadphoneWarning>),
    Unchanged,
}

/// What to do with one reading.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Tell the window.
    Send(HeadphoneWarning),
    /// Same as last time.
    Quiet,
    /// That recording is over: stop reading.
    Stop,
}

impl Tracker {
    pub(crate) const fn new() -> Self {
        Self {
            meeting_id: None,
            shown: None,
        }
    }

    /// The recorder now records `recording` (`None`: not recording).
    pub(crate) fn recorder(&mut self, recording: Option<String>) -> Change {
        match recording {
            Some(id) if self.meeting_id.as_deref() == Some(id.as_str()) => Change::Unchanged,
            Some(id) => {
                self.meeting_id = Some(id.clone());
                self.shown = None;
                Change::Started(id)
            }
            None => match self.meeting_id.take() {
                Some(meeting_id) => {
                    let was_shown = self.shown.take() == Some(true);
                    Change::Stopped(was_shown.then_some(HeadphoneWarning {
                        meeting_id,
                        show: false,
                    }))
                }
                None => Change::Unchanged,
            },
        }
    }

    /// A reading for `meeting_id`'s watch: `show` is whether to warn now.
    pub(crate) fn reading(&mut self, meeting_id: &str, show: bool) -> Step {
        if self.meeting_id.as_deref() != Some(meeting_id) {
            return Step::Stop;
        }
        if self.shown == Some(show) {
            return Step::Quiet;
        }
        self.shown = Some(show);
        Step::Send(HeadphoneWarning {
            meeting_id: meeting_id.to_owned(),
            show,
        })
    }

    /// The warning for the recording in progress, once it has been read.
    pub(crate) fn current(&self) -> Option<HeadphoneWarning> {
        Some(HeadphoneWarning {
            meeting_id: self.meeting_id.clone()?,
            show: self.shown?,
        })
    }
}

static TRACKER: Mutex<Tracker> = Mutex::new(Tracker::new());

/// The meeting a recorder status payload says is recording: `meetingId`
/// when `phase` is `recording`, else `None`.
pub(crate) fn recording_meeting(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    if value.get("phase")?.as_str()? != "recording" {
        return None;
    }
    value.get("meetingId")?.as_str().map(str::to_owned)
}

/// Start following the recorder.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen_any(RECORDING_STATE_EVENT, move |event| {
        let change = lock_or_recover(&TRACKER).recorder(recording_meeting(event.payload()));
        match change {
            Change::Started(meeting_id) => start_watch(&handle, meeting_id),
            Change::Stopped(Some(cleared)) => send(&handle, &cleared),
            Change::Stopped(None) | Change::Unchanged => {}
        }
    });
}

fn start_watch(app: &AppHandle, meeting_id: String) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-headphones".to_owned())
        .spawn(move || watch(&app, &meeting_id));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the headphone check");
    }
}

/// Read the output until `meeting_id`'s recording ends.
fn watch(app: &AppHandle, meeting_id: &str) {
    let enabled = crate::config::warn_no_headphones();
    if !enabled {
        tracing::info!("audio.warn_no_headphones is off: not checking the output");
    }
    loop {
        // Off: no read at all, and one "no banner" answer.
        let kind = if enabled {
            default_output_kind()
        } else {
            OutputKind::Unknown
        };
        let show = should_warn(kind, enabled);
        let step = lock_or_recover(&TRACKER).reading(meeting_id, show);
        match step {
            Step::Stop => return,
            Step::Send(warning) => {
                tracing::info!(output = ?kind, show, "headphone warning");
                send(app, &warning);
            }
            Step::Quiet => {}
        }
        if !enabled {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn send(app: &AppHandle, warning: &HeadphoneWarning) {
    if let Err(error) = app.emit(HEADPHONE_WARNING_EVENT, warning) {
        tracing::warn!(%error, "could not tell the window about headphones");
    }
}

/// The warning for the recording in progress, `None` when nothing is
/// recording or the output has not been read yet.
#[tauri::command]
#[specta::specta]
pub async fn headphone_warning() -> Option<HeadphoneWarning> {
    lock_or_recover(&TRACKER).current()
}

#[cfg(test)]
mod tests;
