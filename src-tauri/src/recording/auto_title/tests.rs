//! Naming a recording against `calendar::fake::FakeProvider` (TUR-29): no
//! Tauri, no Calendar.app, a throwaway meetings folder per test.

use std::path::Path;

use std::sync::Arc;

use ::calendar::CalendarProvider as _;
use ::calendar::fake::FakeProvider;
use ::calendar::raw::{RawAttendee, RawEvent};
use chrono::TimeZone as _;
use meeting_format::layout::MEETING_FILE;
use meeting_format::meeting_md::CALENDAR_EVENT_ID;
use store::meeting::Meeting;

use super::*;

/// The id the recorder gives a recording started at 10:00.
const ID: &str = "2026-10-05-1000-meeting";

impl EventSource for FakeProvider {
    fn events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<::calendar::Event>, ::calendar::Error> {
        self.list_events(from, to)
    }
}

fn at(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 5, h, m, 0).unwrap()
}

fn person(name: &str) -> RawAttendee {
    RawAttendee {
        name: Some(name.into()),
        ..RawAttendee::default()
    }
}

fn event(id: &str, title: &str, start: DateTime<Utc>, minutes: i64, people: &[&str]) -> RawEvent {
    RawEvent {
        id: id.into(),
        title: title.into(),
        start,
        end: start + chrono::Duration::minutes(minutes),
        all_day: false,
        attendees: people.iter().map(|name| person(name)).collect(),
        ical_uid: None,
        join_url: None,
    }
}

fn meetings_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(ID)).unwrap();
    root
}

/// Name the meeting as a recording started at 10:00 would, with the default
/// `min_attendees` of 2.
fn name(root: &Path, provider: &FakeProvider) -> Outcome {
    name_meeting(Some(provider), at(10, 0), 2, |event| {
        store::meeting_event::apply(root, ID, event).map_err(UiError::from)
    })
}

fn meeting(root: &Path) -> Option<Meeting> {
    Meeting::read(&root.join(ID).join(MEETING_FILE)).unwrap()
}

fn named(event_id: &str) -> Outcome {
    Outcome::Named {
        event_id: event_id.into(),
        applied: Applied {
            title: true,
            attendees: true,
            calendar_event_id: true,
        },
    }
}

#[test]
fn one_matching_event_names_the_meeting() {
    let root = meetings_root();
    let provider = FakeProvider::with_events([event(
        "STANDUP-1",
        "Platform Standup",
        at(10, 0),
        15,
        &["Shantanu", "Priya", "Dev"],
    )]);

    assert_eq!(name(root.path(), &provider), named("STANDUP-1"));
    let meeting = meeting(root.path()).unwrap();
    assert_eq!(meeting.title().as_deref(), Some("Platform Standup"));
    assert_eq!(meeting.attendees(), ["Shantanu", "Priya", "Dev"]);
    assert_eq!(
        meeting.frontmatter.get_str(CALENDAR_EVENT_ID).as_deref(),
        Some("STANDUP-1")
    );
    // The folder keeps the id the recorder gave it.
    assert_eq!(meeting.id().as_deref(), Some(ID));
}

#[test]
fn of_two_overlapping_events_the_one_starting_closest_to_now_wins() {
    let root = meetings_root();
    let provider = FakeProvider::with_events([
        event(
            "WORKSHOP",
            "All-day workshop",
            at(9, 0),
            180,
            &["A", "B", "C"],
        ),
        event("SYNC", "Design sync", at(10, 3), 30, &["A", "B"]),
    ]);

    assert_eq!(name(root.path(), &provider), named("SYNC"));
    assert_eq!(
        meeting(root.path()).unwrap().title().as_deref(),
        Some("Design sync")
    );
}

#[test]
fn no_event_now_leaves_the_meeting_untitled() {
    let root = meetings_root();
    // Ended before the recording; starts too late to be this one.
    let provider = FakeProvider::with_events([
        event("EARLIER", "Earlier call", at(9, 0), 30, &["A", "B"]),
        event("LATER", "Later call", at(10, 30), 30, &["A", "B"]),
    ]);

    assert_eq!(name(root.path(), &provider), Outcome::NoMatch);
    assert!(meeting(root.path()).is_none(), "nothing written");
}

#[test]
fn a_solo_block_is_not_a_meeting() {
    let root = meetings_root();
    let provider = FakeProvider::with_events([
        event("FOCUS", "Focus time", at(9, 30), 120, &[]),
        event("ME", "Prep", at(10, 0), 30, &["Shantanu"]),
    ]);

    assert_eq!(name(root.path(), &provider), Outcome::NoMatch);
    assert!(meeting(root.path()).is_none(), "nothing written");
}

#[test]
fn a_hand_typed_title_is_kept() {
    let root = meetings_root();
    let path = root.path().join(ID).join(MEETING_FILE);
    Meeting::new(ID, "Budget review").write(&path).unwrap();
    let provider = FakeProvider::with_events([event(
        "STANDUP-1",
        "Platform Standup",
        at(10, 0),
        15,
        &["Shantanu", "Priya"],
    )]);

    let outcome = name(root.path(), &provider);
    assert_eq!(
        outcome,
        Outcome::Named {
            event_id: "STANDUP-1".into(),
            applied: Applied {
                title: false,
                attendees: true,
                calendar_event_id: true,
            },
        }
    );
    let meeting = meeting(root.path()).unwrap();
    assert_eq!(meeting.title().as_deref(), Some("Budget review"));
    assert_eq!(meeting.attendees(), ["Shantanu", "Priya"]);
}

#[test]
fn a_denied_calendar_is_no_error_and_writes_nothing() {
    let root = meetings_root();
    let outcome = name(root.path(), &FakeProvider::denied());
    assert!(matches!(outcome, Outcome::Unreadable(_)), "{outcome:?}");
    assert!(meeting(root.path()).is_none(), "nothing written");
}

#[test]
fn no_calendar_at_all_writes_nothing() {
    let root = meetings_root();
    let outcome = name_meeting(None, at(10, 0), 2, |event| {
        store::meeting_event::apply(root.path(), ID, event).map_err(UiError::from)
    });
    assert_eq!(outcome, Outcome::NoCalendar);
    assert!(meeting(root.path()).is_none(), "nothing written");
}

#[test]
fn a_failed_write_is_reported_not_raised() {
    let root = tempfile::tempdir().unwrap(); // no meeting folder in it
    let provider = FakeProvider::with_events([event(
        "STANDUP-1",
        "Platform Standup",
        at(10, 0),
        15,
        &["A", "B"],
    )]);
    let outcome = name(root.path(), &provider);
    assert!(
        matches!(&outcome, Outcome::NotWritten { event_id, .. } if event_id == "STANDUP-1"),
        "{outcome:?}"
    );
}

#[test]
fn the_apps_calendar_state_names_the_meeting_through_the_same_path() {
    let root = meetings_root();
    let state = CalendarState::with_providers(vec![Arc::new(FakeProvider::with_events([event(
        "STANDUP-1",
        "Platform Standup",
        at(10, 2),
        15,
        &["A", "B"],
    )]))]);
    let outcome = name_meeting(Some(&state), at(10, 0), 2, |event| {
        store::meeting_event::apply(root.path(), ID, event).map_err(UiError::from)
    });
    assert_eq!(outcome, named("STANDUP-1"));

    let denied = CalendarState::with_providers(vec![Arc::new(FakeProvider::denied())]);
    let outcome = name_meeting(Some(&denied), at(10, 0), 2, |_| {
        unreachable!("nothing to write for a denied calendar")
    });
    assert!(matches!(outcome, Outcome::Unreadable(_)), "{outcome:?}");
}

/// TUR-77: the menu bar's Record names the meeting from the event it was
/// clicked for, even one hours away that `pick_event` would never choose.
#[test]
fn a_pinned_event_names_the_meeting_whenever_it_is() {
    let root = meetings_root();
    let later = ::calendar::raw::to_events(
        [event("LATER", "Later call", at(15, 0), 30, &["A", "B"])],
        at(0, 0),
        at(23, 59),
    );
    let outcome = name_from(&later[0], |event| {
        store::meeting_event::apply(root.path(), ID, event).map_err(UiError::from)
    });
    assert_eq!(outcome, named("LATER"));
    assert_eq!(
        meeting(root.path()).unwrap().title().as_deref(),
        Some("Later call")
    );
}

#[test]
fn a_pin_is_taken_once_and_can_be_cleared() {
    let events = ::calendar::raw::to_events(
        [event("E", "Call", at(10, 0), 30, &["A", "B"])],
        at(0, 0),
        at(23, 59),
    );
    let pin = PinnedEvent::default();
    pin.pin(events[0].clone());
    assert_eq!(pin.take().map(|e| e.id), Some("E".to_string()));
    assert_eq!(pin.take(), None, "taken by one start only");

    pin.pin(events[0].clone());
    pin.clear();
    assert_eq!(pin.take(), None, "a refused start leaves nothing behind");
}
