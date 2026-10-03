//! Naming a recording from the calendar event it belongs to (TUR-29).
//!
//! `docs/problem.md` item 40: the meeting names itself, so the user never types
//! a title. Once a recording is live, [`spawn`] reads the events around its
//! start, lets [`calendar::matching::pick_event`] choose the one being
//! recorded, and has [`store::meeting_event::apply`] write its title,
//! attendees and id into `meeting.md` (SPEC §3.2). The folder keeps its name.
//!
//! None of this can fail a recording. No calendar, a calendar the user did
//! not let meet-ai read, or no event that fits all leave the meeting as it
//! was, untitled, with one debug log line. It runs on a blocking thread of
//! its own, because EventKit can wait up to two minutes for the user to answer
//! the permission prompt, and the recording is already going by then.
//!
//! The calendar is reached through [`EventSource`], so the rules are tested
//! against `calendar::fake::FakeProvider` without Tauri or Calendar.app.

use chrono::{DateTime, Local, Utc};
use store::meeting_event::{Applied, FromCalendar};
use tauri::AppHandle;

use crate::error::UiError;

/// Where the recorder reads calendar events from.
///
/// The app's calendar state implements it; the tests use the fake provider.
pub trait EventSource: Send + Sync {
    /// Events overlapping `[from, to)`. A calendar that cannot be read is an
    /// `Err`, never an empty list.
    fn events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<calendar::Event>, calendar::Error>;
}

/// What came of naming one recording.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// No calendar is set up in the app.
    NoCalendar,
    /// The calendar could not be read (access denied, sign-in expired, ...).
    Unreadable(String),
    /// The calendar was read and no event fits this recording.
    NoMatch,
    /// An event fits; `applied` says which keys `meeting.md` took from it.
    Named { event_id: String, applied: Applied },
    /// An event fits but `meeting.md` could not be written.
    NotWritten { event_id: String, reason: String },
}

/// Name meeting `meeting_id`, whose recording started at `started`, from the
/// calendar, on a thread of its own. Returns at once.
pub(super) fn spawn(app: &AppHandle, meeting_id: &str, started: DateTime<Local>) {
    let app = app.clone();
    let id = meeting_id.to_owned();
    let now = started.with_timezone(&Utc);
    tauri::async_runtime::spawn_blocking(move || {
        let min_attendees = crate::config::detection().min_attendees as usize;
        let outcome = with_source(&app, |source| {
            name_meeting(source, now, min_attendees, |event| {
                crate::folder_move::writing_in_root(&app, |root| {
                    store::meeting_event::apply(root, &id, event).map_err(UiError::from)
                })
            })
        });
        log(&id, &outcome);
    });
}

/// Run `read` with the app's calendar.
///
/// Nothing yet: the app's calendar state (TUR-28) registers here once it is
/// on main. Until then every recording takes the no-calendar path and stays
/// untitled, as before.
fn with_source<R>(_app: &AppHandle, read: impl FnOnce(Option<&dyn EventSource>) -> R) -> R {
    read(None)
}

/// Look up the event a recording starting at `now` belongs to and hand it to
/// `write`. Everything [`spawn`] does, minus Tauri.
pub(crate) fn name_meeting(
    source: Option<&dyn EventSource>,
    now: DateTime<Utc>,
    min_attendees: usize,
    write: impl FnOnce(&FromCalendar<'_>) -> Result<Applied, UiError>,
) -> Outcome {
    let Some(source) = source else {
        return Outcome::NoCalendar;
    };
    let (from, to) = calendar::matching::search_window(now);
    let events = match source.events_between(from, to) {
        Ok(events) => events,
        Err(error) => return Outcome::Unreadable(error.to_string()),
    };
    let Some(event) = calendar::matching::pick_event(&events, now, min_attendees) else {
        return Outcome::NoMatch;
    };
    let fields = FromCalendar {
        event_id: &event.id,
        title: &event.title,
        attendees: &event.attendee_names,
    };
    match write(&fields) {
        Ok(applied) => Outcome::Named {
            event_id: event.id.clone(),
            applied,
        },
        Err(error) => Outcome::NotWritten {
            event_id: event.id.clone(),
            reason: error.message,
        },
    }
}

/// One log line per recording. Only a failed write is worth a warning: no
/// calendar or no match is the ordinary untitled meeting.
fn log(meeting_id: &str, outcome: &Outcome) {
    match outcome {
        Outcome::NoCalendar => {
            tracing::debug!(meeting_id, "no calendar to name the recording from");
        }
        Outcome::Unreadable(reason) => {
            tracing::debug!(meeting_id, %reason, "could not read the calendar; recording stays untitled");
        }
        Outcome::NoMatch => {
            tracing::debug!(
                meeting_id,
                "no calendar event fits the recording; it stays untitled"
            );
        }
        Outcome::Named { event_id, applied } => {
            tracing::info!(meeting_id, %event_id, ?applied, "named the recording from its calendar event");
        }
        Outcome::NotWritten { event_id, reason } => {
            tracing::warn!(meeting_id, %event_id, %reason, "could not write the calendar event into meeting.md");
        }
    }
}

#[cfg(test)]
mod tests;
