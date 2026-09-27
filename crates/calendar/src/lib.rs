//! Calendar access for meet-ai.
//!
//! Phase 5 territory (SPEC §5). Nothing is implemented yet, and the provider
//! dependencies are not even added — see `Cargo.toml`.
//!
//! L13 picks four providers: EventKit (the zero-auth default), Google OAuth,
//! Microsoft Graph OAuth, and a plain ICS URL. [`CalendarProvider`] exists from
//! day one so the UI and the detection logic never branch on which one is in
//! use (SPEC §2.7).

#![forbid(unsafe_op_in_unsafe_fn)]

use chrono::{DateTime, Utc};

/// One calendar entry, flattened to the fields meet-ai actually uses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    /// Stable id within its provider, used to avoid re-notifying.
    pub id: String,
    /// The meeting title. Becomes the recording's auto-title.
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Attendee count, used by the `min_attendees` detection setting.
    pub attendees: usize,
}

/// A source of calendar events.
///
/// Read-only by design — meet-ai never writes to anyone's calendar.
pub trait CalendarProvider {
    /// Human-readable provider name, for the settings screen.
    fn name(&self) -> &'static str;

    /// Events overlapping `[from, to)`.
    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error>;
}

/// Everything that can go wrong reading a calendar.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// macOS has not granted calendar access.
    ///
    /// Like audio permission, this gets a designed screen rather than an error
    /// code, so it is its own variant.
    #[error("meet-ai does not have permission to read your calendar")]
    PermissionDenied,

    /// The stored OAuth token expired and could not be refreshed.
    #[error("your {provider} sign-in has expired and needs renewing")]
    SignInExpired { provider: &'static str },

    /// The provider could not be reached.
    #[error("could not reach {provider}: {detail}")]
    Unreachable {
        provider: &'static str,
        detail: String,
    },
}
