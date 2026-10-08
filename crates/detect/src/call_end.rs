//! The call ended (TUR-144): while recording, notice the call app hanging up,
//! and ask before stopping with a 10-second countdown.
//!
//! Pure state, no clock and no OS: the app's loop hands [`CallEnd`] which apps
//! use the mic (from `audio::mic_users`, every 2 s), whether it is recording,
//! the countdown card's answers and a time, and gets back a [`Step`]. Every
//! rule is a plain unit test with fake readings and a fake clock.
//!
//! The rules (TUR-141 decisions 6 to 10):
//!
//! 1. **Track one app.** A recording started from a call prompt tracks that
//!    prompt's app ([`CallEnd::started_from_prompt`]). Any other recording
//!    (the button, ⌘⇧R, a calendar reminder) tracks the first call app or
//!    browser seen using the mic while it records. None seen, or an OS that
//!    cannot list the apps ([`MicReading::NotSupported`], macOS older than
//!    14): no end prompt ever, only the backup stops (TUR-145).
//! 2. **Off for 5 s straight.** The tracked app must first be seen on the
//!    mic; once it leaves it for [`OFF_WAIT`] in a row, the countdown card
//!    goes up ([`Step::Show`]). A short drop (switching mics, a Bluetooth
//!    headset reconnecting) comes back inside the wait and asks nothing. A
//!    reading that cannot say ([`MicReading::NotSupported`]) changes nothing.
//! 3. **Back on the mic cancels quietly.** During the countdown it closes the
//!    card ([`Step::Cancel`]); the recording goes on, the same recording.
//! 4. **Zero stops.** The countdown running out or **Stop now** is
//!    [`Step::Stop`], the same as pressing Stop.
//! 5. **Keep recording** (or the card closing some other way) asks nothing
//!    more until the app starts and then stops using the mic again.
//! 6. **Stopped by hand** during the countdown closes the card
//!    ([`Step::Close`]).
//!
//! The 5 s wait is measured from the first reading that saw the app gone, so
//! with readings every 2 s the card is up 6 s after the hang-up.

// Adapted from github.com/silverstein/minutes/tauri/src-tauri/src/call_detect.rs @ 9377cb71f215b552b955425dc928920c0648a09c (MIT)
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/mic/macos/app.rs @ 259a04ee2e1447dfed150ed08f0a1bb69909b836 (MIT)

use std::time::{Duration, Instant};

use crate::call_start::{MicKind, MicReading, MicUser};

/// How long the tracked app must stay off the mic before the call counts as
/// ended (TUR-141 decision 6).
pub const OFF_WAIT: Duration = Duration::from_secs(5);

/// How long the countdown card counts before it stops (decision 7).
pub const COUNTDOWN_SECONDS: u32 = 10;

/// How long after the countdown's zero the card's own "timed out" may arrive
/// before [`CallEnd::tick`] stops without it. The card reports zero itself
/// ([`Answer::TimedOut`]); this is only the backstop for a report that never
/// comes, so a card that reached zero on screen never leaves a recording on.
pub const TIMEOUT_GRACE: Duration = Duration::from_secs(1);

/// How long a [`CallEnd::started_from_prompt`] waits for the loop to see
/// the recording it started. Longer than one reading, short enough that a
/// start that failed never names the next recording's app.
pub const ORIGIN_TTL: Duration = Duration::from_secs(30);

/// Is `user` the app called `app` (its id, or its name without case)?
fn is_app(user: &MicUser, app: &str) -> bool {
    user.id == app || user.name.eq_ignore_ascii_case(app)
}

/// How the countdown card ended, as the app's card reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// **Stop now**.
    StopNow,
    /// **Keep recording**.
    KeepRecording,
    /// It reached zero with no answer.
    TimedOut,
    /// It closed without an answer (replaced by another card, or never
    /// shown). Keeps recording.
    Closed,
}

/// What the app should do after a reading, a tick or an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Put the countdown card up: `line` ("Zoom call ended") over
    /// `seconds`. `id` names this countdown in later calls.
    Show {
        id: u32,
        line: String,
        seconds: u32,
    },
    /// The app is back on the mic: close countdown `id` quietly.
    Cancel {
        id: u32,
    },
    /// The recording already stopped: close countdown `id`.
    Close {
        id: u32,
    },
    /// Stop and save the recording, as the Stop button does.
    Stop,
}

/// The card's line for `app`: "Zoom call ended", "Phone call ended",
/// "Call ended in Google Chrome".
pub fn ended_line(app: &MicUser) -> String {
    let name = app.name.trim();
    if app.kind == MicKind::Browser {
        return format!("Call ended in {name}");
    }
    if name.to_lowercase().ends_with(" call") {
        return format!("{name} ended");
    }
    format!("{name} call ended")
}

/// Which apps are on the mic now and were not before, and which were and no
/// longer are, by id.
pub fn diff_apps<'a>(
    previous: &'a [MicUser],
    current: &'a [MicUser],
) -> (Vec<&'a MicUser>, Vec<&'a MicUser>) {
    let started = current
        .iter()
        .filter(|app| !previous.iter().any(|known| known.id == app.id))
        .collect();
    let stopped = previous
        .iter()
        .filter(|app| !current.iter().any(|known| known.id == app.id))
        .collect();
    (started, stopped)
}

/// Where the tracked call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No recording.
    Idle,
    /// Recording, and the tracked app (if one is chosen yet) has not been
    /// seen on the mic.
    Unseen,
    /// The tracked app is using the mic.
    OnMic,
    /// The tracked app left the mic at this reading.
    OffSince(Instant),
    /// Countdown `id` is on screen and reaches zero at `zero`.
    Counting { id: u32, zero: Instant },
    /// **Keep recording**: quiet until the app is back on the mic.
    Kept,
    /// The countdown asked to stop; nothing more until the recording ends.
    Stopping,
}

/// What the call-end rules remember between readings.
#[derive(Debug)]
pub struct CallEnd {
    phase: Phase,
    /// The app this recording follows.
    tracked: Option<MicUser>,
    /// The app a prompt's Record named, waiting for the loop to see the
    /// recording, and when.
    origin: Option<(String, Instant)>,
    /// The app named by the prompt for the recording now running.
    origin_app: Option<String>,
    /// The apps on the mic at the last reading.
    last: Vec<MicUser>,
    last_id: u32,
}

impl Default for CallEnd {
    fn default() -> Self {
        Self::new()
    }
}

impl CallEnd {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            tracked: None,
            origin: None,
            origin_app: None,
            last: Vec::new(),
            last_id: 0,
        }
    }

    /// A prompt's **Record** for `app` (its id or name) just started a
    /// recording at `now`: that recording follows `app`, not the first call
    /// app seen.
    pub fn started_from_prompt(&mut self, app: &str, now: Instant) {
        let app = app.trim();
        if app.is_empty() {
            return;
        }
        if matches!(self.phase, Phase::Idle) {
            self.origin = Some((app.to_string(), now));
            return;
        }
        // The loop already saw the recording: follow `app` from now on,
        // unless a countdown is already up for the app it picked.
        if matches!(
            self.phase,
            Phase::Unseen | Phase::OnMic | Phase::OffSince(_)
        ) {
            self.origin_app = Some(app.to_string());
            self.tracked = self.last.iter().find(|user| is_app(user, app)).cloned();
            self.phase = if self.tracked.is_some() {
                Phase::OnMic
            } else {
                Phase::Unseen
            };
        }
    }

    /// The countdown on screen, if any.
    pub fn counting(&self) -> Option<u32> {
        match self.phase {
            Phase::Counting { id, .. } => Some(id),
            _ => None,
        }
    }

    /// The app this recording follows, once one is seen.
    pub fn tracked(&self) -> Option<&MicUser> {
        self.tracked.as_ref()
    }

    /// A reading at `now`: who uses the mic, and whether meet-ai records.
    pub fn observe(&mut self, reading: &MicReading, recording: bool, now: Instant) -> Step {
        if !recording {
            return self.recording_ended();
        }
        if matches!(self.phase, Phase::Idle) {
            self.begin(now);
        }
        let MicReading::Supported(users) = reading else {
            return Step::Nothing;
        };
        let (started, stopped) = diff_apps(&self.last, users);
        if !started.is_empty() || !stopped.is_empty() {
            tracing::debug!(
                started = ?started.iter().map(|app| &app.name).collect::<Vec<_>>(),
                stopped = ?stopped.iter().map(|app| &app.name).collect::<Vec<_>>(),
                "apps on the mic changed"
            );
        }
        self.last = users.clone();
        if self.tracked.is_none() {
            self.tracked = self.pick(users);
        }
        let Some(tracked) = &self.tracked else {
            return Step::Nothing;
        };
        let on_mic = users.iter().any(|user| user.id == tracked.id);
        match (self.phase, on_mic) {
            (Phase::Unseen | Phase::OnMic | Phase::Kept, true) => {
                self.phase = Phase::OnMic;
                Step::Nothing
            }
            (Phase::OffSince(_), true) => {
                tracing::debug!(app = %tracked.name, "back on the mic before the wait ran out");
                self.phase = Phase::OnMic;
                Step::Nothing
            }
            (Phase::Counting { id, .. }, true) => {
                tracing::info!(app = %tracked.name, "back on the mic: the countdown is cancelled");
                self.phase = Phase::OnMic;
                Step::Cancel { id }
            }
            (Phase::OnMic, false) => {
                self.phase = Phase::OffSince(now);
                self.show_if_waited(now, now)
            }
            (Phase::OffSince(since), false) => self.show_if_waited(since, now),
            // Not seen yet, kept, already counting or stopping: wait.
            (Phase::Unseen | Phase::Kept | Phase::Counting { .. } | Phase::Stopping, false)
            | (Phase::Stopping, true)
            | (Phase::Idle, _) => Step::Nothing,
        }
    }

    /// The countdown's clock at `now`, between readings (every 250 ms
    /// while it is up): closes it when the recording stopped by hand, and
    /// stops if the card's zero never arrived ([`TIMEOUT_GRACE`]).
    pub fn tick(&mut self, recording: bool, now: Instant) -> Step {
        let Phase::Counting { id, zero } = self.phase else {
            return Step::Nothing;
        };
        if !recording {
            self.recording_ended();
            return Step::Close { id };
        }
        if now >= zero + TIMEOUT_GRACE {
            tracing::info!("the countdown reached zero: stopping");
            self.phase = Phase::Stopping;
            return Step::Stop;
        }
        Step::Nothing
    }

    /// Countdown `id` ended with `answer`. A stale id (a countdown already
    /// cancelled or replaced) does nothing.
    pub fn answer(&mut self, id: u32, answer: Answer) -> Step {
        if self.counting() != Some(id) {
            return Step::Nothing;
        }
        match answer {
            Answer::StopNow | Answer::TimedOut => {
                tracing::info!(?answer, "the call ended: stopping");
                self.phase = Phase::Stopping;
                Step::Stop
            }
            Answer::KeepRecording | Answer::Closed => {
                tracing::info!(?answer, "keeping the recording until the call ends again");
                self.phase = Phase::Kept;
                Step::Nothing
            }
        }
    }

    /// A recording was seen at `now` for the first time.
    fn begin(&mut self, now: Instant) {
        self.phase = Phase::Unseen;
        self.tracked = None;
        self.origin_app = self
            .origin
            .take()
            .filter(|(_, at)| now.saturating_duration_since(*at) <= ORIGIN_TTL)
            .map(|(app, _)| app);
    }

    /// The app to follow among `users`: the prompt's, or else the first
    /// call app or browser.
    fn pick(&self, users: &[MicUser]) -> Option<MicUser> {
        let found = match &self.origin_app {
            Some(app) => users.iter().find(|user| is_app(user, app)),
            None => users
                .iter()
                .find(|user| matches!(user.kind, MicKind::CallApp | MicKind::Browser)),
        };
        found.cloned()
    }

    fn show_if_waited(&mut self, since: Instant, now: Instant) -> Step {
        if now.saturating_duration_since(since) < OFF_WAIT {
            return Step::Nothing;
        }
        let Some(tracked) = &self.tracked else {
            return Step::Nothing;
        };
        self.last_id = self.last_id.wrapping_add(1).max(1);
        let id = self.last_id;
        let line = ended_line(tracked);
        tracing::info!(app = %tracked.name, "off the mic for 5 s: asking to stop");
        self.phase = Phase::Counting {
            id,
            zero: now + Duration::from_secs(u64::from(COUNTDOWN_SECONDS)),
        };
        Step::Show {
            id,
            line,
            seconds: COUNTDOWN_SECONDS,
        }
    }

    fn recording_ended(&mut self) -> Step {
        let step = match self.counting() {
            Some(id) => Step::Close { id },
            None => Step::Nothing,
        };
        self.phase = Phase::Idle;
        self.tracked = None;
        self.origin_app = None;
        self.last.clear();
        step
    }
}

#[cfg(test)]
mod tests;
