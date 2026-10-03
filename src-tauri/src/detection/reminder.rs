//! A reminder one minute before each meeting (TUR-30, `docs/problem.md` items
//! 41 and 43).
//!
//! A worker thread ticks every [`TICK`]. [`Reminders::tick`] keeps the next
//! stretch of calendar events (re-read every `calendar.refresh_minutes`) and
//! returns the ones whose reminder is due: [`REMIND_BEFORE`] before their
//! start, with at least `detection.min_attendees` people, so a solo focus
//! block says nothing. Each one goes out as [`detect::Signal::Calendar`]
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
//!   that slept through the minute before a meeting wakes up quiet about it,
//!   and a tick after a long gap re-reads the calendar first, since anything
//!   may have changed while it slept.
//!
//! The clock and the calendar are traits, so the tests run on a fake clock and
//! `calendar::fake::FakeProvider`.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread::JoinHandle;

use ::calendar::{Error, Event};
use chrono::{DateTime, Duration, Utc};

/// How long before a meeting's start its reminder fires.
pub const REMIND_BEFORE: Duration = Duration::seconds(60);

/// How often the loop looks at the clock: well inside [`REMIND_BEFORE`], so a
/// reminder is at most this late while the Mac is awake.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(10);

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
    min_attendees: usize,
    /// `calendar.refresh_minutes`.
    refresh_every: Duration,
    /// The meetings (enough attendees) from the latest successful read.
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
    pub fn new(min_attendees: usize, refresh_every: Duration) -> Self {
        Self {
            min_attendees,
            refresh_every,
            events: Vec::new(),
            read_at: None,
            last_tick: None,
            fired: HashMap::new(),
            unreadable_logged: false,
        }
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

    /// Not started, starting within [`REMIND_BEFORE`], and not reminded
    /// about for this start yet.
    fn is_due(&self, event: &Event, now: DateTime<Utc>) -> bool {
        event.start > now
            && event.start - REMIND_BEFORE <= now
            && self.fired.get(&event.id) != Some(&event.start)
    }

    /// Read from `now` past the next refresh. A failed read keeps the last
    /// good one: an unreadable calendar is not a cancelled meeting.
    fn read(&mut self, now: DateTime<Utc>, calendar: &dyn Upcoming) {
        self.read_at = Some(now);
        let to = now + self.refresh_every + REMIND_BEFORE + LOOKAHEAD_MARGIN;
        match calendar.events_between(now, to) {
            Ok(events) => {
                let min_attendees = self.min_attendees;
                self.events = events
                    .into_iter()
                    .filter(|event| event.attendees >= min_attendees)
                    .collect();
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
/// event to `fire`.
pub fn spawn(
    clock: impl Clock,
    calendar: impl Upcoming,
    mut reminders: Reminders,
    interval: std::time::Duration,
    mut fire: impl FnMut(Event) + Send + 'static,
) -> std::io::Result<ReminderLoop> {
    let (stop, stopped) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("meet-ai-reminders".to_string())
        .spawn(move || {
            loop {
                for event in reminders.tick(clock.now(), &calendar) {
                    fire(event);
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
        #[cfg(target_os = "macos")]
        if ::calendar::eventkit::access() == ::calendar::eventkit::Access::NotAsked {
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
