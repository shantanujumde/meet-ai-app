//! A reminder before each meeting (TUR-30, `docs/problem.md` items 41 and 43),
//! as early as Settings → Notifications says (TUR-78).
//!
//! A worker thread ticks every [`TICK`]. [`Reminders::tick`] keeps the next
//! stretch of calendar events (re-read every `calendar.refresh_minutes`) and
//! returns the ones whose reminder is due: [`ReminderSettings::lead`] before
//! their start (`detection.remind_before_minutes`, default
//! [`DEFAULT_REMIND_BEFORE_MINUTES`]), with at least
//! `detection.min_attendees` people, so a solo focus block says nothing. The
//! loop asks for the settings on every tick, so a change applies without a
//! restart, and with `detection.calendar` off it reads nothing at all. Each one goes out as [`detect::Signal::Calendar`]
//! through the one prompt path, [`super::notify::remind`], whose banner holds
//! **Record** and **Open brief**. Record is the ordinary start-recording path,
//! so the recording names itself from this very event (TUR-29: an event
//! starting within five minutes is the one being recorded).
//!
//! The rules:
//!
//! - **Once per event.** A fired `Event.id` is remembered with the start it
//!   fired for. The same id at the same start never fires again.
//! - **Moved re-arms, cancelled never fires.** A changed start is a new
//!   reminder. Just before anything fires the calendar is read again, so an
//!   event moved or cancelled since the last read is seen as it is now.
//! - **Never late.** Only an event that has not started yet is due. A Mac
//!   that slept through the minutes before a meeting wakes up quiet about it,
//!   and a tick after a long gap re-reads the calendar first, since anything
//!   may have changed while it slept. A lead time of `0` means "at the
//!   start": due from the start for [`AT_START_GRACE`], no later.
//!
//! The clock and the calendar are traits, so the tests run on a fake clock and
//! `calendar::fake::FakeProvider`.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread::JoinHandle;

use ::calendar::{Error, Event};
use chrono::{DateTime, Duration, Utc};

pub use crate::config::DEFAULT_REMIND_BEFORE_MINUTES;

/// How often the loop looks at the clock: well inside a minute, so a
/// reminder is at most this late while the Mac is awake.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(10);

/// With a lead time of `0` ("at the start"), how long after the start the
/// reminder may still fire: two ticks, so one slow tick does not lose it,
/// and never a reminder for a meeting well under way.
pub const AT_START_GRACE: Duration = Duration::seconds(20);

/// What decides which events are due. Read from `detection` on every tick
/// ([`spawn`]'s `settings`), so a Settings change applies at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderSettings {
    /// How long before the start the reminder fires.
    pub lead: Duration,
    /// `detection.min_attendees`.
    pub min_attendees: usize,
}

impl ReminderSettings {
    /// `lead_minutes` before the start, for meetings with `min_attendees`.
    pub fn new(lead_minutes: u32, min_attendees: usize) -> Self {
        Self {
            lead: Duration::minutes(i64::from(lead_minutes)),
            min_attendees,
        }
    }
}

impl Default for ReminderSettings {
    /// The SPEC §3.5 defaults: a minute ahead, two attendees.
    fn default() -> Self {
        Self::new(DEFAULT_REMIND_BEFORE_MINUTES, 2)
    }
}

/// A gap between two ticks longer than this means the Mac slept (or the
/// clock was changed): re-read the calendar before deciding anything.
const WAKE_GAP: Duration = Duration::seconds(30);

/// How long a fired event is remembered past its start. Long enough that it
/// cannot come round again; short enough that a long-running app does not
/// keep every meeting it ever saw.
const FORGET_AFTER: Duration = Duration::days(1);

/// Read past the next refresh by this much, so an event starting just after
/// the next read is already known before its reminder is due.
const LOOKAHEAD_MARGIN: Duration = Duration::minutes(1);

/// Where the time comes from. The real one is [`SystemClock`].
pub trait Clock: Send + 'static {
    fn now(&self) -> DateTime<Utc>;
}

/// The wall clock. Wall time, not a monotonic one: meetings are at wall
/// times, and a monotonic clock stops while the Mac sleeps.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Where upcoming events come from. Blocking; called on the loop's thread.
pub trait Upcoming: Send + 'static {
    /// Events overlapping `[from, to)`. A calendar that cannot be read is an
    /// `Err`, never an empty list.
    fn events_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error>;
}

/// What the reminder loop remembers between ticks.
#[derive(Debug)]
pub struct Reminders {
    settings: ReminderSettings,
    /// `calendar.refresh_minutes`.
    refresh_every: Duration,
    /// The events from the latest successful read. Filtered by attendees when
    /// due, not here, so a changed `min_attendees` needs no re-read.
    events: Vec<Event>,
    /// When the calendar was last read, successfully or not.
    read_at: Option<DateTime<Utc>>,
    last_tick: Option<DateTime<Utc>>,
    /// Every reminded `Event.id`, with the start it was reminded for.
    fired: HashMap<String, DateTime<Utc>>,
    /// The last read failed and that was logged: an unreadable calendar is
    /// one log line until it reads again, not one per refresh.
    unreadable_logged: bool,
}

impl Reminders {
    pub fn new(settings: ReminderSettings, refresh_every: Duration) -> Self {
        Self {
            settings,
            refresh_every,
            events: Vec::new(),
            read_at: None,
            last_tick: None,
            fired: HashMap::new(),
            unreadable_logged: false,
        }
    }

    /// Use `settings` from the next tick on. A longer lead time reaches
    /// further ahead than the last read did, so it re-reads.
    pub fn configure(&mut self, settings: ReminderSettings) {
        if settings.lead > self.settings.lead {
            self.read_at = None;
        }
        self.settings = settings;
    }

    /// One tick at `now`: the events to remind about, each at most once.
    pub fn tick(&mut self, now: DateTime<Utc>, calendar: &dyn Upcoming) -> Vec<Event> {
        let woke = self
            .last_tick
            .is_some_and(|last| now < last || now - last > WAKE_GAP);
        self.last_tick = Some(now);
        if woke {
            tracing::info!("the clock jumped (a sleep, most likely); re-reading the calendar");
        }
        let stale = woke
            || self
                .read_at
                .is_none_or(|at| now < at || now - at >= self.refresh_every);
        if stale {
            self.read(now, calendar);
        }
        if !self.any_due(now) {
            return Vec::new();
        }
        // About to ask: re-read, so a meeting moved or cancelled since the
        // last read is seen as it is now.
        if !stale {
            self.read(now, calendar);
        }
        let due: Vec<Event> = self
            .events
            .iter()
            .filter(|event| self.is_due(event, now))
            .cloned()
            .collect();
        for event in &due {
            self.fired.insert(event.id.clone(), event.start);
        }
        self.fired.retain(|_, start| now - *start < FORGET_AFTER);
        due
    }

    fn any_due(&self, now: DateTime<Utc>) -> bool {
        self.events.iter().any(|event| self.is_due(event, now))
    }

    /// Enough attendees, inside its reminder window ([`due_window`]), and
    /// not reminded about for this start yet.
    fn is_due(&self, event: &Event, now: DateTime<Utc>) -> bool {
        let (from, until) = due_window(event.start, self.settings.lead);
        event.attendees >= self.settings.min_attendees
            && from <= now
            && now < until
            && self.fired.get(&event.id) != Some(&event.start)
    }

    /// Read from `now` past the next refresh. A failed read keeps the last
    /// good one: an unreadable calendar is not a cancelled meeting.
    fn read(&mut self, now: DateTime<Utc>, calendar: &dyn Upcoming) {
        self.read_at = Some(now);
        let to = now + self.refresh_every + self.settings.lead + LOOKAHEAD_MARGIN;
        match calendar.events_between(now, to) {
            Ok(events) => {
                self.events = events;
                self.unreadable_logged = false;
            }
            Err(error) => {
                if !std::mem::replace(&mut self.unreadable_logged, true) {
                    tracing::debug!(%error, "could not read the calendar for meeting reminders");
                }
            }
        }
    }
}

/// When a meeting starting at `start` is due, as `[from, until)`: from
/// `lead` before the start until the start, or with no lead ("at the start")
/// from the start for [`AT_START_GRACE`].
pub fn due_window(start: DateTime<Utc>, lead: Duration) -> (DateTime<Utc>, DateTime<Utc>) {
    if lead > Duration::zero() {
        (start - lead, start)
    } else {
        (start, start + AT_START_GRACE)
    }
}

/// A running reminder loop. Dropping it stops the loop and waits for it.
pub struct ReminderLoop {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl ReminderLoop {
    /// Stop the loop and wait for its thread (at most one calendar read).
    /// The app's loop runs until quit; dropping it does the same.
    #[cfg(test)]
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the meeting reminder thread panicked");
        }
    }
}

impl Drop for ReminderLoop {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Tick `reminders` every `interval` on a thread of its own, handing each due
/// event to `fire`. `settings` is asked on every tick: `None` (reminders
/// switched off) skips the tick without reading the calendar.
pub fn spawn(
    clock: impl Clock,
    calendar: impl Upcoming,
    mut reminders: Reminders,
    interval: std::time::Duration,
    mut settings: impl FnMut() -> Option<ReminderSettings> + Send + 'static,
    mut fire: impl FnMut(Event) + Send + 'static,
) -> std::io::Result<ReminderLoop> {
    let (stop, stopped) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("meet-ai-reminders".to_string())
        .spawn(move || {
            loop {
                if let Some(settings) = settings() {
                    reminders.configure(settings);
                    for event in reminders.tick(clock.now(), &calendar) {
                        fire(event);
                    }
                }
                // Only a timeout keeps going: a stop, or the handle dropped.
                if !matches!(
                    stopped.recv_timeout(interval),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
        })?;
    Ok(ReminderLoop {
        stop: Some(stop),
        thread: Some(thread),
    })
}

/// The app's calendar (TUR-28's `CalendarState`), for the loop.
pub struct AppCalendar(pub tauri::AppHandle);

impl Upcoming for AppCalendar {
    fn events_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        use tauri::Manager as _;

        // A reminder never asks for calendar access: the Today pane does,
        // where the user can see why. Until it has been answered, there is
        // nothing to remind about; the first refresh after a grant reads.
        if !::calendar::eventkit::access_answered() {
            return Err(Error::PermissionDenied);
        }
        match self.0.try_state::<crate::calendar::CalendarState>() {
            Some(state) => state.events_between(from, to),
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests;
