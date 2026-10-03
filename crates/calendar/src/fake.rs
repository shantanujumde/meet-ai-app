//! A [`CalendarProvider`] over fixed events, for tests (SPEC §6).
//!
//! Behind the `fake` cargo feature so it never ships in the app by accident.
//! It runs its events through the same [`to_events`] filter as EventKit, so a
//! test against it sees exactly what a real calendar with those entries would
//! give: no all-day entries, no declined meetings, only the asked-for range.
//!
//! ```
//! use calendar::{CalendarProvider, Error, fake::FakeProvider};
//! # let (from, to) = (chrono::Utc::now(), chrono::Utc::now());
//! assert!(matches!(
//!     FakeProvider::denied().list_events(from, to),
//!     Err(Error::PermissionDenied)
//! ));
//! ```

use chrono::{DateTime, Utc};

use crate::raw::{RawEvent, to_events};
use crate::{CalendarProvider, Error, Event};

/// A calendar with fixed contents, or one that refuses access.
#[derive(Debug, Clone)]
pub struct FakeProvider {
    /// `None` = access denied.
    events: Option<Vec<RawEvent>>,
}

impl FakeProvider {
    /// A calendar the user let meet-ai read, holding `events`.
    pub fn with_events(events: impl IntoIterator<Item = RawEvent>) -> Self {
        Self {
            events: Some(events.into_iter().collect()),
        }
    }

    /// A calendar the user said no to. Every read is
    /// [`Error::PermissionDenied`].
    pub fn denied() -> Self {
        Self { events: None }
    }

    /// Parses a fixture file's JSON: an array of [`RawEvent`]s.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        Ok(Self::with_events(serde_json::from_str::<Vec<RawEvent>>(
            json,
        )?))
    }
}

impl CalendarProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "Fake calendar"
    }

    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error> {
        let events = self.events.as_ref().ok_or(Error::PermissionDenied)?;
        Ok(to_events(events.iter().cloned(), from, to))
    }
}
