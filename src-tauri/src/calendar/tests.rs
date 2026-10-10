use std::sync::Arc;

use ::calendar::fake::FakeProvider;
use ::calendar::raw::{RawAttendee, RawEvent};
use chrono::{DateTime, FixedOffset, Utc};

use super::*;

fn at(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339)
        .expect("valid test date")
        .with_timezone(&Utc)
}

fn person(name: &str) -> RawAttendee {
    RawAttendee {
        name: Some(name.to_string()),
        ..RawAttendee::default()
    }
}

fn raw(id: &str, start: &str, end: &str, people: usize) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        title: format!("Event {id}"),
        start: at(start),
        end: at(end),
        all_day: false,
        attendees: (0..people).map(|i| person(&format!("P{i}"))).collect(),
        ical_uid: None,
        join_url: None,
    }
}

fn shared(provider: FakeProvider) -> SharedProvider {
    Arc::new(provider)
}

const DAY_FROM: &str = "2026-10-03T00:00:00Z";
const DAY_TO: &str = "2026-10-04T00:00:00Z";

#[test]
fn today_is_local_midnight_to_midnight() {
    let ist = FixedOffset::east_opt(5 * 3600 + 1800).expect("valid offset");
    let now = ist
        .with_ymd_and_hms(2026, 10, 3, 18, 33, 0)
        .single()
        .expect("unambiguous");
    let (from, to) = today_bounds(&now);
    assert_eq!(from, at("2026-10-02T18:30:00Z"));
    assert_eq!(to, at("2026-10-03T18:30:00Z"));
}

#[test]
fn events_from_every_provider_come_back_sorted() {
    let state = CalendarState::with_providers(vec![
        shared(FakeProvider::with_events([raw(
            "late",
            "2026-10-03T15:00:00Z",
            "2026-10-03T16:00:00Z",
            3,
        )])),
        shared(FakeProvider::with_events([raw(
            "early",
            "2026-10-03T09:00:00Z",
            "2026-10-03T09:30:00Z",
            2,
        )])),
    ]);
    let events = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect("readable");
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["early", "late"]);
}

#[test]
fn a_denied_calendar_is_an_error_not_an_empty_day() {
    let state = CalendarState::with_providers(vec![shared(FakeProvider::denied())]);
    let error = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect_err("denied");
    assert!(matches!(error, Error::PermissionDenied));

    let ui: UiError = error.into();
    assert_eq!((ui.domain, ui.kind), ("app", "calendar-denied"));
}

#[test]
fn one_readable_provider_is_enough() {
    let state = CalendarState::with_providers(vec![
        shared(FakeProvider::denied()),
        shared(FakeProvider::with_events([raw(
            "a",
            "2026-10-03T09:00:00Z",
            "2026-10-03T10:00:00Z",
            2,
        )])),
    ]);
    let events = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect("one answered");
    assert_eq!(events.len(), 1);
}

#[test]
fn no_providers_is_an_empty_day() {
    let state = CalendarState::with_providers(Vec::new());
    let events = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect("nothing to read");
    assert!(events.is_empty());
}

#[test]
fn below_min_attendees_is_solo() {
    let events = CalendarState::with_providers(vec![shared(FakeProvider::with_events([
        raw("focus", "2026-10-03T08:00:00Z", "2026-10-03T09:00:00Z", 0),
        raw("one", "2026-10-03T09:00:00Z", "2026-10-03T10:00:00Z", 1),
        raw("call", "2026-10-03T10:00:00Z", "2026-10-03T11:00:00Z", 2),
    ]))])
    .events_between(at(DAY_FROM), at(DAY_TO))
    .expect("readable");

    let today = TodaysMeetings::new(events, 15, 2);
    let solo: Vec<(&str, u32, bool)> = today
        .events
        .iter()
        .map(|e| (e.id.as_str(), e.attendees, e.solo))
        .collect();
    assert_eq!(
        solo,
        [("focus", 0, true), ("one", 1, true), ("call", 2, false)]
    );
    assert_eq!((today.refresh_minutes, today.min_attendees), (15, 2));
    assert_eq!(
        today.events[2].start_ms,
        at("2026-10-03T10:00:00Z").timestamp_millis()
    );
}

#[test]
fn the_wire_shape_is_camel_case() {
    let today = TodaysMeetings::new(Vec::new(), 15, 2);
    let json = serde_json::to_value(&today).expect("serializes");
    assert_eq!(json["refreshMinutes"], 15);
    assert_eq!(json["minAttendees"], 2);
}

#[test]
fn every_calendar_error_has_its_own_kind() {
    let expired: UiError = Error::SignInExpired { provider: "Google" }.into();
    let unreachable: UiError = Error::Unreachable {
        provider: "ICS",
        detail: "timed out".into(),
    }
    .into();
    assert_eq!(expired.kind, "calendar-sign-in-expired");
    assert_eq!(unreachable.kind, "calendar-unreachable");
}

#[test]
fn the_state_can_be_managed() {
    fn managed<T: Send + Sync + 'static>() {}
    managed::<CalendarState>();
}

#[test]
fn a_meeting_in_two_calendars_is_read_once_with_the_first_providers_id() {
    // TUR-49: Calendar.app and a Google sign-in both hold the 10:00.
    let mut google = raw("g-1", "2026-10-03T10:00:00Z", "2026-10-03T10:30:00Z", 3);
    google.title = "Event ek-1".into();
    google.ical_uid = Some("uid-1".into());
    let state = CalendarState::with_providers(vec![
        shared(FakeProvider::with_events([raw(
            "ek-1",
            "2026-10-03T10:00:00Z",
            "2026-10-03T10:30:00Z",
            2,
        )])),
        shared(FakeProvider::with_events([
            google,
            raw("g-2", "2026-10-03T12:00:00Z", "2026-10-03T13:00:00Z", 2),
        ])),
    ]);
    let events = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect("readable");
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["ek-1", "g-2"]);
    assert_eq!(events[0].attendees, 3);
    assert_eq!(events[0].ical_uid.as_deref(), Some("uid-1"));
}

/// A sign-in the provider rejected (TUR-174).
struct Expired;

impl CalendarProvider for Expired {
    fn name(&self) -> &'static str {
        "Google"
    }
    fn list_events(&self, _: DateTime<Utc>, _: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        Err(Error::SignInExpired { provider: "Google" })
    }
}

/// Answers only once every provider sharing `barrier` is being read: a
/// sequential read would wait on it forever.
struct WaitsForTheOthers {
    barrier: Arc<std::sync::Barrier>,
    events: FakeProvider,
}

impl CalendarProvider for WaitsForTheOthers {
    fn name(&self) -> &'static str {
        "Waits"
    }
    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        self.barrier.wait();
        self.events.list_events(from, to)
    }
}

#[test]
fn an_expired_sign_in_is_flagged_while_the_other_calendars_still_show() {
    let state = CalendarState::with_providers(vec![
        shared(FakeProvider::with_events([raw(
            "ek-1",
            "2026-10-03T09:00:00Z",
            "2026-10-03T10:00:00Z",
            2,
        )])),
        Arc::new(Expired),
    ]);
    let read = state
        .read_between(at(DAY_FROM), at(DAY_TO))
        .expect("one answered");
    let today = TodaysMeetings::from_read(read, 15, 2);
    let ids: Vec<&str> = today.events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["ek-1"]);
    assert_eq!(
        today.unreadable,
        [UnreadableCalendar {
            provider: "Google".into(),
            kind: "calendar-sign-in-expired".into(),
            message: "your Google sign-in has expired and needs renewing".into(),
        }]
    );
    let json = serde_json::to_value(&today).expect("serializes");
    assert_eq!(json["unreadable"][0]["kind"], "calendar-sign-in-expired");

    // The plain read, for reminders and auto-titles, still skips it.
    let events = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect("one answered");
    assert_eq!(events.len(), 1);
}

#[test]
fn every_calendar_read_is_an_unflagged_day() {
    let read = CalendarState::with_providers(vec![shared(FakeProvider::with_events([]))])
        .read_between(at(DAY_FROM), at(DAY_TO))
        .expect("readable");
    assert!(TodaysMeetings::from_read(read, 15, 2).unreadable.is_empty());
}

#[test]
fn only_failures_are_the_first_error_not_a_flagged_empty_day() {
    let error =
        CalendarState::with_providers(vec![Arc::new(Expired), shared(FakeProvider::denied())])
            .read_between(at(DAY_FROM), at(DAY_TO))
            .expect_err("none answered");
    assert!(matches!(error, Error::SignInExpired { .. }), "{error:?}");
}

#[test]
fn providers_are_read_in_parallel_and_keep_their_order() {
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let waiting = |id: &str, start: &str, end: &str| -> SharedProvider {
        Arc::new(WaitsForTheOthers {
            barrier: Arc::clone(&barrier),
            events: FakeProvider::with_events([raw(id, start, end, 2)]),
        })
    };
    // The same meeting in the first and third: the first's id is kept.
    let mut repeat = raw("g-1", "2026-10-03T09:00:00Z", "2026-10-03T10:00:00Z", 2);
    repeat.title = "Event ek-1".into();
    let state = CalendarState::with_providers(vec![
        waiting("ek-1", "2026-10-03T09:00:00Z", "2026-10-03T10:00:00Z"),
        waiting("ms-1", "2026-10-03T11:00:00Z", "2026-10-03T12:00:00Z"),
        Arc::new(WaitsForTheOthers {
            barrier: Arc::clone(&barrier),
            events: FakeProvider::with_events([repeat]),
        }),
    ]);
    let events = state
        .events_between(at(DAY_FROM), at(DAY_TO))
        .expect("readable");
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["ek-1", "ms-1"]);
}
