//! One list from several calendars, each meeting once (TUR-49).
//!
//! The same meeting is often in more than one source: a Google account added
//! to Calendar.app *and* signed in to directly, or one invite on a work and a
//! personal calendar. [`merge_events`] is the pure function that folds every
//! provider's answer into one sorted list and drops those repeats, so the
//! Today pane, reminders, auto-titles and the brief all see one entry.
//!
//! Two events are the same meeting when they start within
//! [`START_TOLERANCE`] of each other and either:
//!
//! - both carry an iCalendar UID and it is the same, or
//! - their titles match, ignoring case and surrounding spaces.
//!
//! The start is checked even for a UID match: every occurrence of a
//! repeating meeting shares one UID, and Monday's standup is not Tuesday's.
//!
//! Of two copies, the one with more data (named attendees, a join link, a
//! UID) supplies the fields, and the copy seen first keeps its `id`. A join
//! link or UID only the other copy had is kept too. Callers pass
//! EventKit's list first, so an event's id stays the same whether or not a
//! cloud sign-in also holds it: reminders key on that id (TUR-30).

use chrono::Duration;

use crate::Event;

/// How far apart two starts may be and still be one meeting. Providers round
/// differently (Graph to the second, EventKit to the minute).
pub const START_TOLERANCE: Duration = Duration::minutes(1);

/// Every source's events in one list, sorted by start then id, with each
/// meeting once. `sources` is in order of preference: on a repeat, the
/// earlier source's id is kept.
pub fn merge_events(sources: Vec<Vec<Event>>) -> Vec<Event> {
    let mut merged: Vec<Event> = Vec::new();
    for event in sources.into_iter().flatten() {
        match merged.iter_mut().find(|kept| same_meeting(kept, &event)) {
            Some(kept) => absorb(kept, event),
            None => merged.push(event),
        }
    }
    merged.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.id.cmp(&b.id)));
    merged
}

/// Whether `a` and `b` are two copies of one meeting (see the module docs).
pub fn same_meeting(a: &Event, b: &Event) -> bool {
    if (a.start - b.start).abs() > START_TOLERANCE {
        return false;
    }
    let same_uid = matches!(
        (uid(a), uid(b)),
        (Some(left), Some(right)) if left == right
    );
    same_uid || normalized(&a.title) == normalized(&b.title)
}

/// A non-blank UID, trimmed.
fn uid(event: &Event) -> Option<&str> {
    event
        .ical_uid
        .as_deref()
        .map(str::trim)
        .filter(|uid| !uid.is_empty())
}

fn normalized(title: &str) -> String {
    title.trim().to_lowercase()
}

/// How much a copy tells us. More named attendees wins; then a join link,
/// then a UID breaks a tie.
fn richness(event: &Event) -> (usize, bool, bool) {
    (
        event.attendee_names.len(),
        event.join_url.is_some(),
        uid(event).is_some(),
    )
}

/// Fold `other` into `kept`: take the richer copy's fields, keep `kept`'s id,
/// and keep anything only the poorer copy had.
fn absorb(kept: &mut Event, other: Event) {
    let id = std::mem::take(&mut kept.id);
    let attendees = kept.attendees.max(other.attendees);
    if richness(&other) > richness(kept) {
        let ical_uid = kept.ical_uid.take();
        let join_url = kept.join_url.take();
        *kept = other;
        kept.ical_uid = kept.ical_uid.take().or(ical_uid);
        kept.join_url = kept.join_url.take().or(join_url);
    } else {
        if kept.ical_uid.is_none() {
            kept.ical_uid = other.ical_uid;
        }
        if kept.join_url.is_none() {
            kept.join_url = other.join_url;
        }
    }
    kept.id = id;
    kept.attendees = attendees;
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::*;

    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-10-03T{time}Z"))
            .expect("valid test time")
            .with_timezone(&Utc)
    }

    fn event(id: &str, title: &str, start: &str) -> Event {
        let start = at(start);
        Event {
            id: id.into(),
            title: title.into(),
            start,
            end: start + Duration::minutes(30),
            attendees: 2,
            attendee_names: Vec::new(),
            ical_uid: None,
            join_url: None,
        }
    }

    fn with_names(mut event: Event, names: &[&str]) -> Event {
        event.attendee_names = names.iter().map(|name| (*name).to_owned()).collect();
        event.attendees = names.len();
        event
    }

    fn with_uid(mut event: Event, uid: &str) -> Event {
        event.ical_uid = Some(uid.into());
        event
    }

    fn with_link(mut event: Event, url: &str) -> Event {
        event.join_url = Some(url.into());
        event
    }

    fn ids(events: &[Event]) -> Vec<&str> {
        events.iter().map(|event| event.id.as_str()).collect()
    }

    #[test]
    fn one_meeting_in_calendar_app_and_google_is_shown_once() {
        let eventkit = vec![event("ek-1", "Weekly sync", "10:00:00")];
        let google = vec![with_names(
            event("g-1", "  weekly SYNC ", "10:00:30"),
            &["Ada", "Grace", "Linus"],
        )];
        let merged = merge_events(vec![eventkit, google]);
        assert_eq!(merged.len(), 1, "{merged:?}");
        let only = &merged[0];
        // EventKit's id, so reminders keyed on it stay put.
        assert_eq!(only.id, "ek-1");
        // Google's copy named the people, so its fields win.
        assert_eq!(only.attendee_names, ["Ada", "Grace", "Linus"]);
        assert_eq!(only.attendees, 3);
    }

    #[test]
    fn the_same_title_at_different_times_is_two_meetings() {
        let merged = merge_events(vec![
            vec![event("ek-1", "Standup", "09:00:00")],
            vec![event("g-1", "Standup", "09:02:00")],
            vec![event("ms-1", "Standup", "15:00:00")],
        ]);
        assert_eq!(ids(&merged), ["ek-1", "g-1", "ms-1"]);
    }

    #[test]
    fn different_titles_at_the_same_time_are_two_meetings() {
        let merged = merge_events(vec![
            vec![event("ek-1", "Design review", "11:00:00")],
            vec![event("g-1", "Hiring panel", "11:00:00")],
        ]);
        assert_eq!(ids(&merged), ["ek-1", "g-1"]);
    }

    #[test]
    fn the_same_ical_uid_is_one_meeting_even_when_renamed() {
        let merged = merge_events(vec![
            vec![with_uid(event("ek-1", "Q4 planning", "13:00:00"), "uid-42")],
            vec![with_uid(
                event("ms-1", "Q4 planning (moved room)", "13:00:00"),
                "uid-42",
            )],
        ]);
        assert_eq!(ids(&merged), ["ek-1"]);
        assert_eq!(merged[0].ical_uid.as_deref(), Some("uid-42"));
    }

    #[test]
    fn a_shared_uid_on_another_day_is_another_occurrence() {
        let monday = with_uid(event("ek-mon", "Standup", "09:00:00"), "series");
        let mut tuesday = with_uid(event("g-tue", "Standup", "09:00:00"), "series");
        tuesday.start += Duration::days(1);
        tuesday.end += Duration::days(1);
        let merged = merge_events(vec![vec![monday], vec![tuesday]]);
        assert_eq!(ids(&merged), ["ek-mon", "g-tue"]);
    }

    #[test]
    fn a_uid_only_one_copy_has_is_kept() {
        let merged = merge_events(vec![
            vec![with_names(event("ek-1", "1:1", "16:00:00"), &["Ada", "Me"])],
            vec![with_uid(event("g-1", "1:1", "16:00:00"), "uid-7")],
        ]);
        assert_eq!(ids(&merged), ["ek-1"]);
        assert_eq!(merged[0].attendee_names, ["Ada", "Me"]);
        assert_eq!(merged[0].ical_uid.as_deref(), Some("uid-7"));
    }

    #[test]
    fn a_join_link_only_the_poorer_copy_has_is_kept() {
        let link = "https://zoom.us/j/123";
        // EventKit found the link; Google named more people, so its fields win.
        let merged = merge_events(vec![
            vec![with_link(event("ek-1", "Roadmap", "14:00:00"), link)],
            vec![with_names(
                event("g-1", "Roadmap", "14:00:00"),
                &["Ada", "Grace"],
            )],
        ]);
        assert_eq!(ids(&merged), ["ek-1"]);
        assert_eq!(merged[0].attendee_names, ["Ada", "Grace"]);
        assert_eq!(merged[0].join_url.as_deref(), Some(link));

        // And the other way round: the kept copy is richer, the link is on the
        // one folded in.
        let merged = merge_events(vec![
            vec![with_names(
                event("ek-1", "Roadmap", "14:00:00"),
                &["Ada", "Grace"],
            )],
            vec![with_link(event("g-1", "Roadmap", "14:00:00"), link)],
        ]);
        assert_eq!(merged[0].attendee_names, ["Ada", "Grace"]);
        assert_eq!(merged[0].join_url.as_deref(), Some(link));
    }

    #[test]
    fn a_join_link_breaks_a_tie_in_names() {
        let merged = merge_events(vec![
            vec![event("ek-1", "Retro", "17:00:00")],
            vec![with_link(
                with_uid(event("g-1", "Retro", "17:00:00"), "uid-9"),
                "https://meet.google.com/abc-defg-hij",
            )],
        ]);
        assert_eq!(ids(&merged), ["ek-1"]);
        assert_eq!(
            merged[0].join_url.as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
        assert_eq!(merged[0].ical_uid.as_deref(), Some("uid-9"));
    }

    #[test]
    fn the_result_is_sorted_by_start_and_nothing_else_is_dropped() {
        let merged = merge_events(vec![
            vec![
                event("ek-late", "Retro", "17:00:00"),
                event("ek-early", "Standup", "09:00:00"),
            ],
            vec![event("g-mid", "Lunch talk", "12:00:00")],
            Vec::new(),
        ]);
        assert_eq!(ids(&merged), ["ek-early", "g-mid", "ek-late"]);
        assert!(merge_events(Vec::new()).is_empty());
    }
}
