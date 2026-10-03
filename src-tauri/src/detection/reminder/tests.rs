//! The reminder rules on a fake clock and `calendar::fake::FakeProvider`: no
//! Tauri, no Calendar.app, no real time passing (except the one loop test,
//! whose clock is still fake).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ::calendar::CalendarProvider as _;
use ::calendar::fake::FakeProvider;
use ::calendar::raw::{RawAttendee, RawEvent};
use chrono::TimeZone as _;

use super::*;

fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 5, h, m, s).unwrap()
}

fn people(n: usize) -> Vec<RawAttendee> {
    (0..n)
        .map(|i| RawAttendee {
            name: Some(format!("Person {i}")),
            ..RawAttendee::default()
        })
        .collect()
}

fn invite(id: &str, start: DateTime<Utc>, attendees: usize) -> RawEvent {
    RawEvent {
        id: id.into(),
        title: format!("{id} title"),
        start,
        end: start + Duration::minutes(30),
        all_day: false,
        attendees: people(attendees),
        ical_uid: None,
        join_url: None,
    }
}

/// A calendar the test can edit between ticks, counting reads, or failing.
#[derive(Clone, Default)]
struct Calendar {
    events: Arc<Mutex<Vec<RawEvent>>>,
    failing: Arc<Mutex<bool>>,
    reads: Arc<AtomicUsize>,
}

impl Calendar {
    fn with(events: Vec<RawEvent>) -> Self {
        let calendar = Self::default();
        calendar.set(events);
        calendar
    }

    fn set(&self, events: Vec<RawEvent>) {
        *self.events.lock().unwrap() = events;
    }

    fn fail(&self, failing: bool) {
        *self.failing.lock().unwrap() = failing;
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl Upcoming for Calendar {
    fn events_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if *self.failing.lock().unwrap() {
            return Err(Error::PermissionDenied);
        }
        FakeProvider::with_events(self.events.lock().unwrap().clone()).list_events(from, to)
    }
}

fn reminders() -> Reminders {
    // The SPEC §3.5 defaults: two attendees, a read every 15 minutes.
    Reminders::new(2, Duration::minutes(15))
}

/// Tick every [`TICK`] over `[from, to]`, returning each firing's time and id.
fn run(
    reminders: &mut Reminders,
    calendar: &Calendar,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, String)> {
    let step = Duration::from_std(TICK).unwrap();
    let mut fired = Vec::new();
    let mut now = from;
    while now <= to {
        for event in reminders.tick(now, calendar) {
            fired.push((now, event.id));
        }
        now += step;
    }
    fired
}

#[test]
fn fires_once_at_t_minus_60s() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let mut reminders = reminders();
    let fired = run(&mut reminders, &calendar, at(9, 50, 0), at(10, 30, 0));
    assert_eq!(fired, [(at(9, 59, 0), "standup".to_string())]);
}

#[test]
fn the_reminder_says_who_and_what() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let fired = reminders().tick(at(9, 59, 0), &calendar);
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].title, "standup title");
    assert_eq!(fired[0].attendees, 3);
}

#[test]
fn a_solo_block_stays_silent() {
    let calendar = Calendar::with(vec![
        invite("focus", at(10, 0, 0), 1),
        invite("alone", at(11, 0, 0), 0),
    ]);
    let mut reminders = reminders();
    assert!(run(&mut reminders, &calendar, at(9, 50, 0), at(11, 30, 0)).is_empty());

    // `min_attendees` decides: three wanted, two invited, still silent.
    let calendar = Calendar::with(vec![invite("pair", at(10, 0, 0), 2)]);
    let mut strict = Reminders::new(3, Duration::minutes(15));
    assert!(run(&mut strict, &calendar, at(9, 50, 0), at(10, 30, 0)).is_empty());
}

#[test]
fn a_moved_event_re_arms() {
    let calendar = Calendar::with(vec![invite("sync", at(10, 0, 0), 2)]);
    let mut reminders = reminders();
    assert_eq!(
        run(&mut reminders, &calendar, at(9, 50, 0), at(9, 59, 50)),
        [(at(9, 59, 0), "sync".to_string())]
    );
    // Pushed back half an hour, same id: it asks again before the new start.
    calendar.set(vec![invite("sync", at(10, 30, 0), 2)]);
    assert_eq!(
        run(&mut reminders, &calendar, at(10, 0, 0), at(10, 45, 0)),
        [(at(10, 29, 0), "sync".to_string())]
    );
}

#[test]
fn a_move_seen_just_before_the_reminder_fires_at_the_new_time() {
    let calendar = Calendar::with(vec![invite("sync", at(10, 0, 0), 2)]);
    let mut reminders = reminders();
    // Read at 9:50; moved at 9:55, no refresh due until 10:05.
    assert!(reminders.tick(at(9, 50, 0), &calendar).is_empty());
    calendar.set(vec![invite("sync", at(10, 10, 0), 2)]);
    let fired = run(&mut reminders, &calendar, at(9, 50, 10), at(10, 15, 0));
    assert_eq!(fired, [(at(10, 9, 0), "sync".to_string())]);
}

#[test]
fn a_cancelled_event_never_fires() {
    let calendar = Calendar::with(vec![invite("retro", at(10, 0, 0), 4)]);
    let mut reminders = reminders();
    // Known since 9:50, cancelled at 9:58, before the 10:05 refresh.
    assert!(run(&mut reminders, &calendar, at(9, 50, 0), at(9, 58, 0)).is_empty());
    calendar.set(Vec::new());
    assert!(run(&mut reminders, &calendar, at(9, 58, 10), at(10, 30, 0)).is_empty());
}

#[test]
fn an_unreadable_calendar_keeps_the_last_read() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let mut reminders = reminders();
    assert!(reminders.tick(at(9, 50, 0), &calendar).is_empty());
    calendar.fail(true);
    let fired = run(&mut reminders, &calendar, at(9, 50, 10), at(10, 5, 0));
    assert_eq!(fired, [(at(9, 59, 0), "standup".to_string())]);
}

#[test]
fn a_calendar_granted_later_is_read_on_the_next_refresh() {
    let calendar = Calendar::with(vec![invite("standup", at(9, 20, 0), 3)]);
    calendar.fail(true);
    let mut reminders = reminders();
    assert!(run(&mut reminders, &calendar, at(9, 0, 0), at(9, 10, 0)).is_empty());
    assert_eq!(
        calendar.reads(),
        1,
        "an unreadable calendar waits for the refresh"
    );
    calendar.fail(false);
    let fired = run(&mut reminders, &calendar, at(9, 10, 10), at(9, 25, 0));
    assert_eq!(fired, [(at(9, 19, 0), "standup".to_string())]);
    assert!(
        !reminders.unreadable_logged,
        "a good read clears the logged failure"
    );
}

#[test]
fn sleeping_across_the_reminder_skips_it() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let mut reminders = reminders();
    assert!(run(&mut reminders, &calendar, at(9, 50, 0), at(9, 58, 50)).is_empty());
    // Asleep from 9:58:50 to 10:20: the meeting has started, so no late
    // reminder, then or later.
    assert!(run(&mut reminders, &calendar, at(10, 20, 0), at(11, 0, 0)).is_empty());
}

#[test]
fn waking_before_the_start_still_reminds() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let mut reminders = reminders();
    assert!(reminders.tick(at(9, 0, 0), &calendar).is_empty());
    let fired = run(&mut reminders, &calendar, at(9, 59, 30), at(10, 10, 0));
    assert_eq!(fired, [(at(9, 59, 30), "standup".to_string())]);
}

#[test]
fn waking_re_reads_the_calendar() {
    let calendar = Calendar::with(Vec::new());
    let mut reminders = reminders();
    assert!(reminders.tick(at(9, 0, 0), &calendar).is_empty());
    assert!(reminders.tick(at(9, 0, 10), &calendar).is_empty());
    assert_eq!(calendar.reads(), 1, "no read until the refresh is due");
    // Booked while the Mac slept, starting a minute after it wakes.
    calendar.set(vec![invite("booked", at(9, 6, 30), 2)]);
    assert_eq!(reminders.tick(at(9, 5, 40), &calendar)[0].id, "booked");
}

#[test]
fn the_calendar_is_read_once_per_refresh_and_once_before_asking() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let mut reminders = reminders();
    run(&mut reminders, &calendar, at(9, 30, 0), at(9, 58, 50));
    // 9:30 and 9:45.
    assert_eq!(calendar.reads(), 2);
    run(&mut reminders, &calendar, at(9, 59, 0), at(9, 59, 50));
    assert_eq!(
        calendar.reads(),
        3,
        "re-read once, just before the reminder"
    );
}

#[test]
fn an_event_just_past_the_next_refresh_is_already_known() {
    // Read at 9:00 (next read 9:15); the event starts at 9:16, so its
    // reminder at 9:15 must not wait for that read to have happened first.
    let calendar = Calendar::with(vec![invite("early", at(9, 16, 0), 2)]);
    let mut reminders = reminders();
    let fired = run(&mut reminders, &calendar, at(9, 0, 0), at(9, 20, 0));
    assert_eq!(fired, [(at(9, 15, 0), "early".to_string())]);
}

#[test]
fn two_meetings_at_once_each_remind() {
    let calendar = Calendar::with(vec![
        invite("a", at(10, 0, 0), 2),
        invite("b", at(10, 0, 0), 5),
    ]);
    let mut reminders = reminders();
    let fired = run(&mut reminders, &calendar, at(9, 58, 0), at(10, 1, 0));
    assert_eq!(
        fired,
        [
            (at(9, 59, 0), "a".to_string()),
            (at(9, 59, 0), "b".to_string())
        ]
    );
}

#[test]
fn a_meeting_already_running_at_launch_is_not_reminded() {
    let calendar = Calendar::with(vec![invite("late", at(10, 0, 0), 3)]);
    let mut reminders = reminders();
    assert!(run(&mut reminders, &calendar, at(10, 0, 0), at(10, 30, 0)).is_empty());
}

#[test]
fn record_from_the_reminder_names_the_recording_after_this_event() {
    // Record goes through the normal start path, whose auto-title (TUR-29)
    // picks the event at the moment recording starts: the reminder's moment.
    let calendar = Calendar::with(vec![
        invite("workshop", at(9, 0, 0), 8),
        invite("standup", at(10, 0, 0), 3),
    ]);
    let fired = reminders().tick(at(9, 59, 0), &calendar);
    let (from, to) = ::calendar::matching::search_window(at(9, 59, 0));
    let events = calendar.events_between(from, to).unwrap();
    let picked = ::calendar::matching::pick_event(&events, at(9, 59, 0), 2).unwrap();
    assert_eq!(picked.id, fired[0].id);
}

/// A clock the test sets.
#[derive(Clone)]
struct FakeClock(Arc<Mutex<DateTime<Utc>>>);

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

#[test]
fn the_loop_fires_each_reminder_once() {
    let calendar = Calendar::with(vec![invite("standup", at(10, 0, 0), 3)]);
    let clock = FakeClock(Arc::new(Mutex::new(at(9, 59, 0))));
    let (tx, fired) = mpsc::channel();
    let running = spawn(
        clock,
        calendar.clone(),
        reminders(),
        std::time::Duration::from_millis(1),
        move |event| {
            let _ = tx.send(event.id);
        },
    )
    .unwrap();
    assert_eq!(
        fired
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap(),
        "standup"
    );
    // Dozens more ticks at the same moment: nothing more.
    std::thread::sleep(std::time::Duration::from_millis(50));
    running.stop();
    assert!(fired.try_recv().is_err());
    // One read: the first was fresh enough to ask from.
    assert_eq!(calendar.reads(), 1);
}
