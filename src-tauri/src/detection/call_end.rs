//! The call ended (TUR-144): while recording, follow the call app on the mic,
//! and when it hangs up show "Zoom call ended" counting down from 10 with
//! **Stop now** and **Keep recording** (TUR-147's countdown card).
//!
//! The rules are `detect::call_end` (pure, tested with fake readings and a
//! fake clock). This file only feeds them and carries out what they say:
//!
//! * [`start`] runs a loop that, every 2 s while recording with
//!   `detection.call_end` on, reads which apps use the mic
//!   (`audio::mic_users`, TUR-142) and hands that to the rules. Off, or
//!   not recording, nothing is read. Where the OS cannot list the apps
//!   (macOS older than 14) the rules never see a call app, so there is no
//!   end prompt: only the backup stops apply.
//! * A countdown runs on its own thread ([`drive`]) at 250 ms ticks: the
//!   card's answer, the recording stopping by hand (the card closes) and the
//!   rules cancelling it (the app is back on the mic) are all seen there.
//! * Stopping is the Stop button's own path, `Recorder::stop`; nothing else
//!   stops here.
//! * A prompt's **Record** names its app ([`prompted_app`],
//!   [`started_from_prompt`]), so that recording follows that app rather
//!   than the first call app seen.

// Adapted from github.com/silverstein/minutes/tauri/src-tauri/src/call_detect.rs @ 9377cb71f215b552b955425dc928920c0648a09c (MIT)

use std::sync::Mutex;
use std::time::{Duration, Instant};

use audio::mic_users::{AppKind, MicUsers};
use detect::call_end::{Answer, CallEnd, MicKind, MicReading, MicUser, Step};
use tauri::{AppHandle, Manager as _};

use super::Detection;
use super::popup::countdown::{self, CountdownEnd, CountdownHandle};
use super::popup::{Card, PopupAnswer, PopupPrompt};
use crate::lock::lock_or_recover;
use crate::recording::Recorder;

/// How often the apps on the mic are read while recording.
pub const POLL: Duration = detect::AUDIO_POLL_INTERVAL;

/// How often a countdown checks its card, the recording and the rules.
pub const TICK: Duration = Duration::from_millis(250);

/// The call-end rules, shared by the loop and the countdown thread.
#[derive(Debug, Default)]
pub struct Watch(Mutex<CallEnd>);

impl Watch {
    fn with<T>(&self, f: impl FnOnce(&mut CallEnd) -> T) -> T {
        f(&mut lock_or_recover(&self.0))
    }
}

/// Start following calls while recording. The loop lives as long as the app.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("meet-ai-call-end".to_string())
        .spawn(move || {
            loop {
                poll(&app);
                std::thread::sleep(POLL);
            }
        });
    match spawned {
        Ok(_) => tracing::info!(interval = ?POLL, "watching for the call ending while recording"),
        Err(error) => tracing::warn!(%error, "could not start the call-end watcher"),
    }
}

/// One reading: who uses the mic (only while recording with the switch on).
fn poll(app: &AppHandle) {
    let Some(state) = app.try_state::<Detection>() else {
        return;
    };
    let recording = super::notify::recording(app);
    let reading = if recording && state.config().call_end {
        reading(audio::mic_users::mic_users())
    } else {
        MicReading::NotSupported
    };
    let step = state
        .call_end
        .with(|rules| rules.observe(&reading, recording, Instant::now()));
    act(app, step);
}

/// Carry out what the rules said.
fn act(app: &AppHandle, step: Step) {
    match step {
        Step::Nothing => {}
        Step::Show { id, line, seconds } => {
            let shown = app.clone();
            let spawned = std::thread::Builder::new()
                .name("meet-ai-call-end-countdown".to_string())
                .spawn(move || countdown(&shown, id, &line, seconds));
            if let Err(error) = spawned {
                tracing::warn!(%error, "could not run the call-ended countdown");
                settle(app_rules(app), id, Answer::Closed);
            }
        }
        // The countdown's thread sees the rules move on and closes its card.
        Step::Cancel { id } | Step::Close { id } => {
            tracing::debug!(id, "closing the call-ended countdown");
        }
        Step::Stop => stop(app),
    }
}

/// The call-end rules, if the detection state is there.
fn app_rules(app: &AppHandle) -> Option<tauri::State<'_, Detection>> {
    app.try_state::<Detection>()
}

fn settle(state: Option<tauri::State<'_, Detection>>, id: u32, answer: Answer) {
    if let Some(state) = state {
        state.call_end.with(|rules| rules.answer(id, answer));
    }
}

/// Show countdown `id` and follow it to its end.
fn countdown(app: &AppHandle, id: u32, line: &str, seconds: u32) {
    let Some(state) = app_rules(app) else {
        return;
    };
    let Some(handle) = countdown::show_countdown(app, line, seconds) else {
        // A card nobody saw never stops a recording.
        tracing::warn!("could not show the call-ended countdown; keeping the recording");
        settle(Some(state), id, Answer::Closed);
        return;
    };
    let stops = drive(
        &handle,
        id,
        &state.call_end,
        || super::notify::recording(app),
        Instant::now,
        || std::thread::sleep(TICK),
    );
    if stops {
        stop(app);
    }
}

/// What a countdown card needs to do for [`drive`]: say how it ended, and
/// close quietly. The real one is TUR-147's [`CountdownHandle`].
pub trait CountdownCard {
    fn try_end(&self) -> Option<CountdownEnd>;
    fn cancel(&self);
}

impl CountdownCard for CountdownHandle {
    fn try_end(&self) -> Option<CountdownEnd> {
        CountdownHandle::try_end(self)
    }
    fn cancel(&self) {
        CountdownHandle::cancel(self);
    }
}

/// Follow countdown `id` on `card` until it ends, a tick at a time: its
/// answer, the recording stopping by hand, the rules cancelling it, or its
/// zero. `true` when the recording should stop. The card is closed when the
/// rules end it first.
pub fn drive(
    card: &impl CountdownCard,
    id: u32,
    rules: &Watch,
    recording: impl Fn() -> bool,
    clock: impl Fn() -> Instant,
    mut wait: impl FnMut(),
) -> bool {
    loop {
        wait();
        let step = match card.try_end() {
            Some(end) => rules.with(|rules| rules.answer(id, answer_for(end))),
            None => {
                let recording = recording();
                rules.with(|rules| rules.tick(recording, clock()))
            }
        };
        let stops = step == Step::Stop;
        if stops || rules.with(|rules| rules.counting()) != Some(id) {
            // A no-op when the card already closed itself.
            card.cancel();
            return stops;
        }
    }
}

/// The rules' answer for how the card ended.
pub fn answer_for(end: CountdownEnd) -> Answer {
    match end {
        CountdownEnd::StopNow => Answer::StopNow,
        CountdownEnd::KeepRecording => Answer::KeepRecording,
        CountdownEnd::TimedOut => Answer::TimedOut,
        CountdownEnd::Closed => Answer::Closed,
    }
}

/// Stop and save, as the Stop button does.
fn stop(app: &AppHandle) {
    let Some(recorder) = app.try_state::<Recorder>() else {
        return;
    };
    tracing::info!("the call ended: stopping the recording");
    if let Err(error) = recorder.stop(app) {
        tracing::warn!(message = %error.message, "the recording did not stop cleanly");
    }
}

/// The apps on the mic, as the rules see them. Dictation, audio-routing
/// tools and system daemons are never a call, so they are left out.
pub fn reading(users: MicUsers) -> MicReading {
    match users {
        MicUsers::NotSupported => MicReading::NotSupported,
        MicUsers::Supported(apps) => MicReading::Supported(
            apps.into_iter()
                .filter_map(|app| {
                    let kind = match app.kind {
                        AppKind::CallApp => MicKind::CallApp,
                        AppKind::Browser => MicKind::Browser,
                        AppKind::Other => MicKind::Other,
                        AppKind::IgnoredSystem => return None,
                    };
                    Some(MicUser::new(app.id, app.name, kind))
                })
                .collect(),
        ),
    }
}

/// The app a popup answer starts recording for: the one the prompt on screen
/// names, when `answer` on card `id` is **Record**.
pub fn prompted_app(shown: Option<&PopupPrompt>, id: u32, answer: PopupAnswer) -> Option<String> {
    if !matches!(answer, PopupAnswer::Record | PopupAnswer::JoinAndRecord) {
        return None;
    }
    let shown = shown.filter(|shown| shown.id == id)?;
    match &shown.card {
        Card::Prompt { prompt } if !prompt.test => prompt.app.clone(),
        Card::Prompt { .. } | Card::Countdown { .. } => None,
    }
}

/// A prompt's **Record** for `name` started the recording: follow that app.
pub fn started_from_prompt(app: &AppHandle, name: Option<&str>) {
    let (Some(name), Some(state)) = (name, app_rules(app)) else {
        return;
    };
    state
        .call_end
        .with(|rules| rules.started_from_prompt(name, Instant::now()));
}

#[cfg(test)]
mod tests;
