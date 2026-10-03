//! Which calendar event a recording that starts now belongs to (TUR-29).
//!
//! `docs/problem.md` item 40: the meeting names itself from the calendar, so
//! the user never types a title. When a recording starts, the app reads the
//! events in [`search_window`] and asks [`pick_event`] for the one it is
//! recording. Pure functions over [`Event`]s, so every rule is tested without
//! a calendar.

use chrono::{DateTime, Duration, Utc};

use crate::Event;

/// How far ahead an event may start and still be the one being recorded:
/// people start recording a few minutes early.
pub const START_SOON: Duration = Duration::minutes(5);

/// The `[from, to)` range to read events for, for a recording starting at
/// `now`: every event [`pick_event`] could choose overlaps it.
///
/// `to` is one second past `now + START_SOON` because providers read a
/// half-open range, and an event starting exactly [`START_SOON`] from now
/// still counts.
pub fn search_window(now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    (now, now + START_SOON + Duration::seconds(1))
}

/// The event a recording starting at `now` belongs to, if any.
///
/// An event qualifies when it is running at `now` (started at or before it and
/// not yet ended) or starts within [`START_SOON`] after it, and it has at least
/// `min_attendees` attendees, so a solo focus block never names a call. When
/// several qualify, the one whose start is closest to `now` wins, either side;
/// an exact tie goes to the earlier start, then to the smaller id, so the
/// answer never depends on the order the provider listed them in.
pub fn pick_event(events: &[Event], now: DateTime<Utc>, min_attendees: usize) -> Option<&Event> {
    events
        .iter()
        .filter(|e| e.attendees >= min_attendees && is_now_or_soon(e, now))
        .min_by(|a, b| {
            distance(a, now)
                .cmp(&distance(b, now))
                .then_with(|| a.start.cmp(&b.start))
                .then_with(|| a.id.cmp(&b.id))
        })
}

/// Running at `now`, or starting within [`START_SOON`] of it.
fn is_now_or_soon(event: &Event, now: DateTime<Utc>) -> bool {
    let running = event.start <= now && event.end > now;
    let soon = event.start > now && event.start <= now + START_SOON;
    running || soon
}

/// How far the event's start is from `now`, either way.
fn distance(event: &Event, now: DateTime<Utc>) -> Duration {
    (event.start - now).abs()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, h, m, 0).unwrap()
    }

    fn event(id: &str, start: DateTime<Utc>, end: DateTime<Utc>, attendees: usize) -> Event {
        Event {
            id: id.into(),
            title: id.into(),
            start,
            end,
            attendees,
            attendee_names: Vec::new(),
            ical_uid: None,
            join_url: None,
        }
    }

    fn picked(events: &[Event], now: DateTime<Utc>) -> Option<&str> {
        pick_event(events, now, 2).map(|e| e.id.as_str())
    }

    #[test]
    fn a_running_event_is_picked() {
        let events = [event("call", at(10, 0), at(10, 30), 3)];
        assert_eq!(picked(&events, at(10, 10)), Some("call"));
        assert_eq!(picked(&events, at(10, 0)), Some("call"));
    }

    #[test]
    fn an_event_starting_within_five_minutes_is_picked_and_one_later_is_not() {
        let events = [event("call", at(10, 0), at(10, 30), 3)];
        assert_eq!(picked(&events, at(9, 55)), Some("call"));
        assert_eq!(picked(&events, at(9, 54)), None);
    }

    #[test]
    fn an_event_that_has_ended_is_not_picked() {
        let events = [event("call", at(9, 0), at(9, 30), 3)];
        assert_eq!(picked(&events, at(9, 30)), None);
    }

    #[test]
    fn too_few_attendees_is_not_a_meeting() {
        let events = [event("focus", at(10, 0), at(11, 0), 1)];
        assert_eq!(picked(&events, at(10, 5)), None);
        assert_eq!(
            pick_event(&events, at(10, 5), 1).map(|e| e.id.as_str()),
            Some("focus")
        );
    }

    #[test]
    fn of_two_overlapping_events_the_start_closest_to_now_wins() {
        // A long workshop still running, and the standup about to start.
        let events = [
            event("workshop", at(9, 0), at(12, 0), 8),
            event("standup", at(10, 3), at(10, 15), 4),
        ];
        assert_eq!(picked(&events, at(10, 0)), Some("standup"));
        // Ten minutes in, the standup's start (7 min ago) is nearer than the
        // workshop's (70 min ago).
        assert_eq!(picked(&events, at(10, 10)), Some("standup"));
    }

    #[test]
    fn closeness_counts_both_ways() {
        let events = [
            event("late", at(10, 4), at(10, 30), 2),
            event("running", at(9, 58), at(10, 30), 2),
        ];
        assert_eq!(picked(&events, at(10, 0)), Some("running"));
    }

    #[test]
    fn an_exact_tie_does_not_depend_on_list_order() {
        let a = event("a", at(9, 58), at(10, 30), 2);
        let b = event("b", at(10, 2), at(10, 30), 2);
        let c = event("c", at(10, 2), at(10, 45), 2);
        for events in [[a.clone(), b.clone(), c.clone()], [c, b, a]] {
            assert_eq!(picked(&events, at(10, 0)), Some("a"));
        }
    }

    #[test]
    fn the_search_window_covers_every_event_that_can_be_picked() {
        let now = at(10, 0);
        let (from, to) = search_window(now);
        assert_eq!(from, now);
        assert!(now + START_SOON < to);
    }
}
