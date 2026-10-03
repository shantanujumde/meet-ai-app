//! The provider-neutral event shape, and the one function that filters it.
//!
//! A provider reads its source into [`RawEvent`]s — plain data, no framework
//! types — and hands them to [`to_events`]. Everything that decides which
//! events meet-ai shows lives here, so it is unit-tested without EventKit and
//! every provider applies the same rules.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::Event;

/// One calendar entry as a provider read it, before filtering.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawEvent {
    /// The provider's id for this occurrence; see [`occurrence_id`].
    pub id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// An all-day entry (a holiday, a birthday, "out of office"). Never a call.
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub attendees: Vec<RawAttendee>,
    /// The iCalendar UID, when the provider gives one; see [`Event::ical_uid`].
    #[serde(default)]
    pub ical_uid: Option<String>,
}

/// One invitee as a provider read it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RawAttendee {
    /// Display name, if the provider has one.
    pub name: Option<String>,
    /// Email address, without any `mailto:` prefix.
    pub email: Option<String>,
    /// This invitee is the person using meet-ai.
    pub is_me: bool,
    /// This invitee declined.
    pub declined: bool,
    /// A room or other bookable resource, not a person.
    pub resource: bool,
}

/// Turns raw entries into the [`Event`]s meet-ai shows for `[from, to)`.
///
/// Drops all-day entries, entries the user declined, and entries outside the
/// range; sorts what is left by start time (ties by id, so the order is
/// stable). Rooms and resources are neither counted nor named.
pub fn to_events(
    raw: impl IntoIterator<Item = RawEvent>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<Event> {
    let mut events: Vec<Event> = raw
        .into_iter()
        .filter(|e| !e.all_day && !declined_by_me(e) && overlaps(e, from, to))
        .map(|e| {
            let people: Vec<&RawAttendee> = e.attendees.iter().filter(|a| !a.resource).collect();
            Event {
                attendees: people.len(),
                attendee_names: people.iter().filter_map(|a| display_name(a)).collect(),
                id: e.id,
                title: e.title,
                start: e.start,
                end: e.end,
                ical_uid: e.ical_uid,
            }
        })
        .collect();
    events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.id.cmp(&b.id)));
    events
}

/// The user said no. An event with no "me" among its attendees (one the user
/// made for themself, or a calendar that does not list them) is not declined.
fn declined_by_me(event: &RawEvent) -> bool {
    event.attendees.iter().any(|a| a.is_me && a.declined)
}

/// Half-open overlap with `[from, to)`. A zero-length event counts when its
/// instant is inside the range.
fn overlaps(event: &RawEvent, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    event.start < to && (event.end > from || (event.end == event.start && event.start >= from))
}

/// Name, else email, else nothing. Blank strings count as missing.
fn display_name(attendee: &RawAttendee) -> Option<String> {
    [&attendee.name, &attendee.email]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The email address in a participant URL like `mailto:priya@example.com`.
///
/// Calendar servers spell the scheme in either case. Anything that is not a
/// `mailto:` URL (a `urn:uuid:` principal, an `http` link) has no address.
pub fn email_from_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("mailto") {
        return None;
    }
    // `mailto:a@b?subject=x` is legal; only the address part is wanted.
    let address = rest.split('?').next().unwrap_or_default().trim();
    (!address.is_empty()).then(|| address.to_owned())
}

/// An id that is unique per occurrence.
///
/// Calendar stores give every occurrence of a repeating meeting the same id.
/// Detection dedupes reminders by id, so a shared id would mean yesterday's
/// standup silences today's. `occurrence` is the occurrence's *original* start
/// (stable even if that one occurrence is moved), or `None` for a one-off
/// meeting, whose id is left as is.
pub fn occurrence_id(base: &str, occurrence: Option<DateTime<Utc>>) -> String {
    match occurrence {
        Some(at) => format!("{base}@{}", at.timestamp()),
        None => base.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, h, m, 0).single().unwrap()
    }

    fn event(id: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> RawEvent {
        RawEvent {
            id: id.into(),
            title: format!("title {id}"),
            start,
            end,
            all_day: false,
            attendees: vec![],
            ical_uid: None,
        }
    }

    fn person(name: Option<&str>, email: Option<&str>) -> RawAttendee {
        RawAttendee {
            name: name.map(Into::into),
            email: email.map(Into::into),
            ..RawAttendee::default()
        }
    }

    fn day() -> (DateTime<Utc>, DateTime<Utc>) {
        (at(0, 0), at(23, 59))
    }

    #[test]
    fn keeps_a_plain_meeting_with_every_field() {
        let mut e = event("a", at(9, 0), at(9, 30));
        e.attendees = vec![person(Some("Priya"), Some("priya@example.com"))];
        let (from, to) = day();
        assert_eq!(
            to_events([e], from, to),
            vec![Event {
                id: "a".into(),
                title: "title a".into(),
                start: at(9, 0),
                end: at(9, 30),
                attendees: 1,
                attendee_names: vec!["Priya".into()],
                ical_uid: None,
            }]
        );
    }

    #[test]
    fn skips_all_day_entries() {
        let mut e = event("holiday", at(0, 0), at(23, 59));
        e.all_day = true;
        let (from, to) = day();
        assert!(to_events([e], from, to).is_empty());
    }

    #[test]
    fn skips_an_event_i_declined() {
        let mut e = event("no", at(9, 0), at(10, 0));
        e.attendees = vec![RawAttendee {
            is_me: true,
            declined: true,
            ..person(Some("Me"), None)
        }];
        let (from, to) = day();
        assert!(to_events([e], from, to).is_empty());
    }

    #[test]
    fn keeps_an_event_someone_else_declined() {
        let mut e = event("yes", at(9, 0), at(10, 0));
        e.attendees = vec![
            RawAttendee {
                is_me: true,
                ..person(Some("Me"), None)
            },
            RawAttendee {
                declined: true,
                ..person(Some("Dev"), None)
            },
        ];
        let (from, to) = day();
        let events = to_events([e], from, to);
        assert_eq!(events.len(), 1);
        // A decliner is still an invitee; the count is about who was asked.
        assert_eq!(events[0].attendees, 2);
    }

    #[test]
    fn names_fall_back_to_email_and_blank_names_do_not_count() {
        let mut e = event("n", at(9, 0), at(10, 0));
        e.attendees = vec![
            person(Some("Priya"), Some("priya@example.com")),
            person(None, Some("dev@example.com")),
            person(Some("  "), Some("sam@example.com")),
            person(None, None),
        ];
        let (from, to) = day();
        let events = to_events([e], from, to);
        assert_eq!(events[0].attendees, 4);
        assert_eq!(
            events[0].attendee_names,
            vec!["Priya", "dev@example.com", "sam@example.com"]
        );
    }

    #[test]
    fn rooms_are_neither_counted_nor_named() {
        let mut e = event("r", at(9, 0), at(10, 0));
        e.attendees = vec![
            person(Some("Priya"), None),
            RawAttendee {
                resource: true,
                ..person(Some("Room 4B"), None)
            },
        ];
        let (from, to) = day();
        let events = to_events([e], from, to);
        assert_eq!(events[0].attendees, 1);
        assert_eq!(events[0].attendee_names, vec!["Priya"]);
    }

    #[test]
    fn range_is_half_open() {
        let (from, to) = (at(9, 0), at(10, 0));
        let ends_at_from = event("before", at(8, 0), at(9, 0));
        let starts_at_to = event("after", at(10, 0), at(11, 0));
        let straddles = event("straddle", at(8, 30), at(9, 30));
        let instant = event("instant", at(9, 0), at(9, 0));
        let ids: Vec<String> =
            to_events([ends_at_from, starts_at_to, straddles, instant], from, to)
                .into_iter()
                .map(|e| e.id)
                .collect();
        assert_eq!(ids, vec!["straddle", "instant"]);
    }

    #[test]
    fn sorted_by_start_then_id() {
        let (from, to) = day();
        let ids: Vec<String> = to_events(
            [
                event("c", at(11, 0), at(12, 0)),
                event("b", at(9, 0), at(10, 0)),
                event("a", at(9, 0), at(9, 15)),
            ],
            from,
            to,
        )
        .into_iter()
        .map(|e| e.id)
        .collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn email_from_url_reads_mailto_only() {
        assert_eq!(
            email_from_url("mailto:priya@example.com").as_deref(),
            Some("priya@example.com")
        );
        assert_eq!(
            email_from_url("MAILTO:dev@example.com?subject=hi").as_deref(),
            Some("dev@example.com")
        );
        assert_eq!(email_from_url("mailto:"), None);
        assert_eq!(email_from_url("urn:uuid:1234"), None);
        assert_eq!(email_from_url("no-scheme"), None);
    }

    #[test]
    fn occurrence_id_is_unique_per_occurrence() {
        assert_eq!(occurrence_id("ABC", None), "ABC");
        let monday = occurrence_id("ABC", Some(at(9, 0)));
        let tuesday = occurrence_id("ABC", Some(at(9, 0) + chrono::Duration::days(1)));
        assert_ne!(monday, tuesday);
        assert_eq!(monday, format!("ABC@{}", at(9, 0).timestamp()));
    }
}
