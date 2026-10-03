//! `cargo test -p calendar` against fixture events, with no Calendar.app
//! (SPEC §6, Phase 5a). The fixture day is 2026-10-05 UTC.

use calendar::fake::FakeProvider;
use calendar::{CalendarProvider, Error};
use chrono::{DateTime, TimeZone, Utc};

const FIXTURE: &str = include_str!("fixtures/events.json");

fn day(d: u32) -> (DateTime<Utc>, DateTime<Utc>) {
    let from = Utc.with_ymd_and_hms(2026, 10, d, 0, 0, 0).single().unwrap();
    (from, from + chrono::Duration::days(1))
}

fn fixture() -> FakeProvider {
    FakeProvider::from_json(FIXTURE).expect("fixture parses")
}

#[test]
fn todays_meetings_skip_all_day_and_declined() {
    let (from, to) = day(5);
    let events = fixture().list_events(from, to).unwrap();
    let titles: Vec<&str> = events.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "Platform Standup",
            "Focus time",
            "Design review (Google calendar)"
        ]
    );
}

#[test]
fn fields_are_mapped() {
    let (from, to) = day(5);
    let events = fixture().list_events(from, to).unwrap();

    let standup = &events[0];
    assert_eq!(standup.id, "STANDUP-SERIES@1791181800");
    assert_eq!(
        standup.start,
        Utc.with_ymd_and_hms(2026, 10, 5, 4, 30, 0).unwrap()
    );
    assert_eq!(
        standup.end,
        Utc.with_ymd_and_hms(2026, 10, 5, 4, 45, 0).unwrap()
    );
    assert_eq!(standup.attendees, 3);
    assert_eq!(
        standup.attendee_names,
        vec!["Me", "Priya", "dev@example.com"]
    );

    let focus = &events[1];
    assert_eq!(focus.attendees, 0);
    assert!(focus.attendee_names.is_empty());

    // The room is not a person; Priya declined but was still invited.
    let review = &events[2];
    assert_eq!(review.attendees, 2);
    assert_eq!(review.attendee_names, vec!["Me", "Priya"]);
}

#[test]
fn another_day_has_its_own_events() {
    let (from, to) = day(6);
    let events = fixture().list_events(from, to).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "Retro");
}

#[test]
fn an_empty_day_is_ok_and_empty() {
    let (from, to) = day(7);
    assert!(fixture().list_events(from, to).unwrap().is_empty());
}

/// The ticket's core rule: "can't read your calendar" is never an empty list,
/// so the caller can tell it apart from "no meetings today".
#[test]
fn denied_is_an_error_not_an_empty_day() {
    let (from, to) = day(5);
    let result = FakeProvider::denied().list_events(from, to);
    assert!(matches!(result, Err(Error::PermissionDenied)), "{result:?}");

    let empty = FakeProvider::with_events([]).list_events(from, to);
    assert!(matches!(empty.as_deref(), Ok([])), "{empty:?}");
}

#[test]
fn the_provider_works_as_a_trait_object() {
    let providers: Vec<Box<dyn CalendarProvider>> =
        vec![Box::new(fixture()), Box::new(FakeProvider::denied())];
    let (from, to) = day(5);
    let results: Vec<_> = providers.iter().map(|p| p.list_events(from, to)).collect();
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}
