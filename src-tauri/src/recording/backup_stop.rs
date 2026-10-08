//! Backup stops (TUR-145): the recording ends even when nobody presses Stop.
//!
//! A call app that keeps the mic after hang-up, or someone who walked away,
//! leaves a recording running for hours. Two stops catch that, for every
//! recording however it started (a detected call, the calendar, ⌘⇧R):
//!
//! * **Ten minutes of silence.** No speech on the mic or in the call for
//!   [`silence::SILENCE_LIMIT`] puts up TUR-147's countdown card, "No one has
//!   spoken for 10 minutes", with **Stop now** and **Keep recording**; it
//!   stops when the ring reaches zero. Keep recording starts the ten minutes
//!   again. Off with Settings → Notifications, "Stop after 10 min of
//!   silence" (`detection.stop_after_silence`). Speech is judged by the
//!   transcript's own detector ([`speech`]) on a copy of each channel's
//!   frames of its own, fanned out ahead of the live transcript's tees, so
//!   it works with `transcription.live` off too.
//! * **Sleep.** The computer going to sleep or its lid closing stops and
//!   saves the recording at once, no card ([`platform`]). Always on.
//!
//! Both stop through [`Recorder::stop`], the Stop button's own path, after
//! noting why ([`recording_state::note_backup_stop`]), so the meeting says
//! that it ended because of sleep or silence
//! ([`recording_state::RecordingState`]). Nothing here starts a recording,
//! and there is no stop when a calendar event ends.

mod listen;
mod platform;
mod silence;
mod speech;

use std::sync::Arc;
use std::time::{Duration, Instant};

use audio::session::{RecordingSession, Tees};
use audio::tee::TeeFeed;
use tauri::{AppHandle, Manager as _};

use self::listen::{Card, Channel, Ended, World};
use self::silence::{SILENCE_LIMIT, SilenceTimer};
use self::speech::SpeechGate;
use super::start_check::{self, Check};
use super::{Phase, Recorder};
use crate::detection::Detection;
use crate::detection::popup::CountdownEnd;
use crate::detection::popup::countdown::{CountdownHandle, show_countdown};
use crate::recording_state::{self, BackupStop};

/// The silence card's line. The ring under it counts the seconds.
const SILENCE_LINE: &str = "No one has spoken for 10 minutes";

/// How long the silence card counts down before it stops the recording.
const COUNTDOWN_SECONDS: u32 = 10;

/// How often the silence loop reads its channels.
const POLL: Duration = Duration::from_millis(250);

/// The loop's thread name, next to `meet-ai-recording-ticker` in a sample.
const THREAD_NAME: &str = "meet-ai-silence-stop";

/// [`start_check::tees`], plus a copy of each channel for the silence stop.
///
/// The copy's tee is the outer one and hands every frame on to the tee it
/// wraps, so the live transcript's tees and the permission check's fan-out
/// see exactly what they saw before. It is not hushed: the check's chime
/// counts as a moment of sound, which only ever delays the question.
pub(super) fn tees() -> (Tees, TeeFeed, TeeFeed, Checks) {
    let (mut tees, mic_feed, sys_feed, check) = start_check::tees();
    let (mic_copy, mic_heard) = audio::tee::tee();
    let (sys_copy, sys_heard) = audio::tee::tee();
    tees.mic = tees.mic.map(|tee| mic_copy.fan_out(tee));
    tees.sys = tees.sys.map(|tee| sys_copy.fan_out(tee));
    let checks = Checks {
        check,
        heard: [mic_heard, sys_heard],
    };
    (tees, mic_feed, sys_feed, checks)
}

/// What runs alongside a recording: the system-audio check (TUR-136) and the
/// silence stop, in the shape `recording.rs` already starts the check.
pub(super) struct Checks {
    check: Check,
    heard: [TeeFeed; 2],
}

impl Checks {
    /// Point the system-audio check at the session.
    pub(super) fn attach(self, session: &RecordingSession) -> Self {
        Self {
            check: self.check.attach(session),
            ..self
        }
    }

    /// Start both on their own threads and return at once.
    pub(super) fn spawn(self, app: &AppHandle) {
        let Self { check, heard } = self;
        check.spawn(app);
        spawn_silence_stop(app, heard);
    }
}

fn spawn_silence_stop(app: &AppHandle, heard: [TeeFeed; 2]) {
    let Some(meeting_id) = app.state::<Recorder>().status().meeting_id else {
        return;
    };
    recording_state::forget_backup_stop(&meeting_id);
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let mut channels = heard.map(|feed| Channel::new(feed, SpeechGate::earshot()));
            let mut timer = SilenceTimer::new(Instant::now(), SILENCE_LIMIT);
            let mut world = Live {
                app: app.clone(),
                meeting_id: meeting_id.clone(),
            };
            let mut card = SilenceCard {
                app: app.clone(),
                up: None,
            };
            if listen::listen(&mut channels, &mut timer, &mut world, &mut card) == Ended::Stop {
                tracing::info!("no one spoke for 10 minutes; stopping the recording");
                stop_for(&app, &meeting_id, BackupStop::Silence);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the silence stop; this recording has none");
    }
}

/// The recorder and the setting, for the silence loop.
struct Live {
    app: AppHandle,
    meeting_id: String,
}

impl World for Live {
    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn enabled(&mut self) -> bool {
        self.app
            .try_state::<Detection>()
            .is_none_or(|detection| detection.config().stop_after_silence)
    }

    fn still_recording(&mut self) -> bool {
        is_recording(&self.app, &self.meeting_id)
    }

    fn pause(&mut self) {
        std::thread::sleep(POLL);
    }
}

/// TUR-147's countdown card, for the silence loop.
struct SilenceCard {
    app: AppHandle,
    up: Option<CountdownHandle>,
}

impl Card for SilenceCard {
    fn show(&mut self) -> bool {
        self.up = show_countdown(&self.app, SILENCE_LINE, COUNTDOWN_SECONDS);
        if self.up.is_none() {
            tracing::warn!("could not show the silence card; the recording goes on");
        }
        self.up.is_some()
    }

    fn poll(&mut self) -> Option<CountdownEnd> {
        let end = self
            .up
            .as_ref()
            .map_or(Some(CountdownEnd::Closed), |up| up.try_end());
        if end.is_some() {
            self.up = None;
        }
        end
    }

    fn withdraw(&mut self) {
        if let Some(up) = self.up.take() {
            up.cancel();
        }
    }
}

/// Whether meeting `meeting_id` is the one recording now.
fn is_recording(app: &AppHandle, meeting_id: &str) -> bool {
    app.try_state::<Recorder>().is_some_and(|recorder| {
        let status = recorder.status();
        status.phase == Phase::Recording && status.meeting_id.as_deref() == Some(meeting_id)
    })
}

/// Stop meeting `meeting_id`'s recording the way the Stop button does,
/// noting `why` first. Nothing when it is no longer the one recording.
fn stop_for(app: &AppHandle, meeting_id: &str, why: BackupStop) {
    if !is_recording(app, meeting_id) {
        return;
    }
    recording_state::note_backup_stop(meeting_id, why);
    match app.state::<Recorder>().stop(app) {
        Ok(_) => tracing::info!(meeting_id, reason = why.as_str(), "recording stopped"),
        Err(error) => tracing::error!(
            message = %error.message,
            reason = why.as_str(),
            "the backup stop could not finish the recording cleanly"
        ),
    }
}

/// Stop and save the recording whenever the computer is about to sleep
/// (TUR-145). Called once at startup; a hook the OS will not give is logged
/// and the app runs on without it.
pub(crate) fn install_sleep_stop(app: &AppHandle) {
    let app = app.clone();
    let handler: platform::Handler = Arc::new(move || {
        let live = app
            .try_state::<Recorder>()
            .and_then(|recorder| recorder.status().meeting_id);
        if let Some(meeting_id) = live {
            stop_for(&app, &meeting_id, BackupStop::Sleep);
        }
    });
    match platform::on_will_sleep(handler) {
        Ok(()) => tracing::debug!("recordings stop when the computer sleeps"),
        Err(error) => tracing::warn!(%error, "recordings will not stop for sleep"),
    }
}
