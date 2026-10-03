//! Tier 1: EventKit, every account already in Calendar.app (SPEC §2.7).
//!
//! iCloud, Google, Exchange, Outlook and CalDAV accounts the user added to
//! Calendar.app all come through here behind one macOS permission prompt, with
//! no OAuth. This is the only file in the crate with OS-specific code (SPEC
//! §8.2). [`EventKitProvider`] exists on every OS, so the app builds it
//! without a `#[cfg]` of its own; off macOS every read is
//! [`Error::Unreachable`], since there is no Calendar.app to read. Everything
//! else here is macOS only.
//!
//! The prompt text is `NSCalendarsFullAccessUsageDescription` in
//! `src-tauri/Info.plist`. macOS remembers the answer per signed bundle id, so
//! only a signed build (`just bundle-signed`) shows it; `tauri dev` cannot.
//!
//! Reading is synchronous and can wait for the user to answer the prompt.
//! Call [`EventKitProvider::list_events`] and [`request_access`] off the main
//! thread (a `spawn_blocking` task, a worker thread).

#[cfg(target_os = "macos")]
use std::sync::mpsc;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use block2::RcBlock;
use chrono::{DateTime, Utc};
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::Bool;
#[cfg(target_os = "macos")]
use objc2_event_kit::{
    EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStore, EKParticipant, EKParticipantStatus,
    EKParticipantType,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSDate, NSError};

#[cfg(target_os = "macos")]
use crate::raw::{RawAttendee, RawEvent, email_from_url, occurrence_id, to_events};
use crate::{CalendarProvider, Error, Event};

/// The provider's name, for the settings screen and error messages.
const NAME: &str = "Calendar (this Mac)";

/// How long [`request_access`] waits for the user to answer the prompt.
/// Past it the answer counts as "no" for this read; the next read asks again.
#[cfg(target_os = "macos")]
const PROMPT_WAIT: Duration = Duration::from_secs(120);

/// Where meet-ai stands with calendar permission.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Never asked. The next read shows the prompt.
    NotAsked,
    /// Full read access.
    Granted,
    /// The user said no (or turned it off in System Settings).
    Denied,
    /// A device-management profile forbids it; the user cannot change it.
    Restricted,
    /// The user granted add-only access, which cannot read events.
    WriteOnly,
}

#[cfg(target_os = "macos")]
impl Access {
    fn from_status(status: EKAuthorizationStatus) -> Self {
        match status {
            EKAuthorizationStatus::FullAccess => Self::Granted,
            EKAuthorizationStatus::Denied => Self::Denied,
            EKAuthorizationStatus::Restricted => Self::Restricted,
            EKAuthorizationStatus::WriteOnly => Self::WriteOnly,
            // NotDetermined, and any status a later macOS adds: ask.
            _ => Self::NotAsked,
        }
    }
}

/// The current calendar permission, without prompting.
#[cfg(target_os = "macos")]
pub fn access() -> Access {
    // SAFETY: a class method taking a plain enum; callable from any thread.
    let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
    Access::from_status(status)
}

/// Whether the calendar permission has been answered either way, so a read
/// will not show the macOS prompt. On every OS, so callers need no `#[cfg]`
/// of their own (SPEC §8.2): off macOS there is no prompt, so `true` (and
/// every read is [`Error::Unreachable`] anyway).
pub fn access_answered() -> bool {
    #[cfg(target_os = "macos")]
    {
        access() != Access::NotAsked
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Makes sure meet-ai may read events, showing the macOS prompt if it has
/// never been answered. Blocks until the user answers (at most
/// [`PROMPT_WAIT`]).
///
/// Uses the macOS 14+ full-access request; the older `requestAccessToEntityType`
/// is deprecated since macOS 14.
#[cfg(target_os = "macos")]
pub fn request_access() -> Result<(), Error> {
    match access() {
        Access::Granted => return Ok(()),
        Access::Denied | Access::Restricted | Access::WriteOnly => {
            return Err(Error::PermissionDenied);
        }
        Access::NotAsked => {}
    }

    let (tx, rx) = mpsc::channel::<bool>();
    let completion: RcBlock<dyn Fn(Bool, *mut NSError)> =
        RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            // The receiver may have timed out and gone; nothing to tell then.
            let _ = tx.send(granted.as_bool());
        });
    // SAFETY: `EKEventStore::new` is a plain `+new`. The completion block is
    // a valid heap block that EventKit copies and calls once, on a queue of
    // its choosing; it only sends on a channel, which is thread-safe.
    unsafe {
        let store = EKEventStore::new();
        store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&completion));
    }
    match rx.recv_timeout(PROMPT_WAIT) {
        Ok(true) => Ok(()),
        // Said no, or did not answer in time.
        Ok(false) | Err(_) => Err(Error::PermissionDenied),
    }
}

/// Reads every calendar in Calendar.app.
///
/// Holds no state: each read makes its own `EKEventStore`, which keeps the
/// provider `Send + Sync` and means a grant given since the last read is
/// always seen.
///
/// Off macOS it reads nothing: every read is [`Error::Unreachable`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EventKitProvider;

impl EventKitProvider {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(target_os = "macos"))]
impl CalendarProvider for EventKitProvider {
    fn name(&self) -> &'static str {
        NAME
    }

    fn list_events(&self, _from: DateTime<Utc>, _to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        Err(Error::Unreachable {
            provider: NAME,
            detail: "Calendar.app is only on macOS".to_string(),
        })
    }
}

#[cfg(target_os = "macos")]
impl CalendarProvider for EventKitProvider {
    fn name(&self) -> &'static str {
        NAME
    }

    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        request_access()?;
        if from >= to {
            return Ok(Vec::new());
        }
        let start = ns_date(from);
        let end = ns_date(to);
        // SAFETY: access is granted (checked above), so the store has its
        // calendars. The predicate comes from this same store's factory
        // method, which `eventsMatchingPredicate` requires. `None` = every
        // calendar.
        let found = unsafe {
            let store = EKEventStore::new();
            let predicate =
                store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);
            store.eventsMatchingPredicate(&predicate)
        };
        let raw: Vec<RawEvent> = found.iter().filter_map(|e| raw_event(&e)).collect();
        Ok(to_events(raw, from, to))
    }
}

#[cfg(target_os = "macos")]
fn ns_date(at: DateTime<Utc>) -> Retained<NSDate> {
    let secs = at.timestamp() as f64 + f64::from(at.timestamp_subsec_millis()) / 1000.0;
    NSDate::dateWithTimeIntervalSince1970(secs)
}

#[cfg(target_os = "macos")]
fn utc(date: &NSDate) -> Option<DateTime<Utc>> {
    let millis = (date.timeIntervalSince1970() * 1000.0).round();
    // An out-of-range date (NaN, or past year 262143) is not a meeting.
    if !millis.is_finite() || millis.abs() > i64::MAX as f64 {
        return None;
    }
    DateTime::from_timestamp_millis(millis as i64)
}

#[cfg(target_os = "macos")]
/// One `EKEvent` as plain data. `None` for an entry with an unreadable date.
fn raw_event(event: &EKEvent) -> Option<RawEvent> {
    // SAFETY: plain property getters on an event this store just returned,
    // read on the thread that fetched it.
    unsafe {
        let start = utc(&event.startDate())?;
        let end = utc(&event.endDate())?;
        let base = event
            .eventIdentifier()
            .map(|id| id.to_string())
            .unwrap_or_else(|| event.calendarItemIdentifier().to_string());
        let repeating = event.hasRecurrenceRules() || event.isDetached();
        let occurrence = repeating
            .then(|| event.occurrenceDate())
            .flatten()
            .and_then(|d| utc(&d));
        let attendees = event
            .attendees()
            .map(|list| list.iter().map(|p| raw_attendee(&p)).collect())
            .unwrap_or_default();
        Some(RawEvent {
            id: occurrence_id(&base, occurrence),
            title: event.title().to_string(),
            start,
            end,
            all_day: event.isAllDay(),
            attendees,
        })
    }
}

#[cfg(target_os = "macos")]
fn raw_attendee(p: &EKParticipant) -> RawAttendee {
    // SAFETY: plain property getters on a participant of a fetched event.
    unsafe {
        let kind = p.participantType();
        RawAttendee {
            name: p.name().map(|n| n.to_string()),
            email: p
                .URL()
                .absoluteString()
                .and_then(|u| email_from_url(&u.to_string())),
            is_me: p.isCurrentUser(),
            declined: p.participantStatus() == EKParticipantStatus::Declined,
            resource: kind == EKParticipantType::Room || kind == EKParticipantType::Resource,
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn answered_is_anything_but_not_asked() {
        // A status read: no prompt, whatever this Mac has granted.
        assert_eq!(access_answered(), access() != Access::NotAsked);
    }

    #[test]
    fn every_status_that_cannot_read_is_not_granted() {
        assert_eq!(
            Access::from_status(EKAuthorizationStatus::FullAccess),
            Access::Granted
        );
        assert_eq!(
            Access::from_status(EKAuthorizationStatus::Denied),
            Access::Denied
        );
        assert_eq!(
            Access::from_status(EKAuthorizationStatus::Restricted),
            Access::Restricted
        );
        assert_eq!(
            Access::from_status(EKAuthorizationStatus::WriteOnly),
            Access::WriteOnly
        );
        assert_eq!(
            Access::from_status(EKAuthorizationStatus::NotDetermined),
            Access::NotAsked
        );
    }

    #[test]
    fn dates_round_trip_through_nsdate() {
        let at = DateTime::from_timestamp_millis(1_790_000_000_123).unwrap();
        assert_eq!(utc(&ns_date(at)), Some(at));
    }

    #[test]
    #[ignore = "needs a calendar permission grant"]
    fn reads_todays_events_from_calendar_app() {
        let now = Utc::now();
        let events = EventKitProvider::new()
            .list_events(
                now - chrono::Duration::hours(12),
                now + chrono::Duration::hours(12),
            )
            .unwrap();
        for e in &events {
            assert!(e.start <= e.end, "{e:?}");
        }
        eprintln!("{} events", events.len());
    }
}
