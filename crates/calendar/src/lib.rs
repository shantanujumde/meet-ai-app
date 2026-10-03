//! Calendar access for meet-ai.
//!
//! L13 picks four providers: EventKit (the zero-auth default), Google OAuth,
//! Microsoft Graph OAuth, and a plain ICS URL. [`CalendarProvider`] exists from
//! day one so the UI and the detection logic never branch on which one is in
//! use (SPEC §2.7).
//!
//! What is here (Phase 5a, TUR-26):
//!
//! - [`eventkit`]: tier 1, every account already in Calendar.app, behind one
//!   macOS permission prompt. It is the only file in this crate with OS-specific
//!   code (SPEC §8.2): off macOS only [`eventkit::EventKitProvider`] exists,
//!   and every read is an error.
//! - [`raw`]: the plain, provider-neutral shape a provider reads first, and the
//!   one pure function that turns it into [`Event`]s (skip all-day, skip
//!   declined, name the attendees). Shared so every provider filters the same
//!   way, and so the filtering is tested without EventKit.
//! - `fake` (cargo feature `fake`): a provider over fixture events, for tests
//!   that must not touch Calendar.app (SPEC §6).

#![forbid(unsafe_op_in_unsafe_fn)]

use chrono::{DateTime, Utc};

pub mod eventkit;
#[cfg(feature = "fake")]
pub mod fake;
pub mod matching;
pub mod raw;

/// One calendar entry, flattened to the fields meet-ai actually uses.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    /// Stable id within its provider, used to avoid re-notifying.
    ///
    /// Each occurrence of a repeating meeting has its own id (see
    /// [`raw::occurrence_id`]), so yesterday's standup does not silence
    /// today's.
    pub id: String,
    /// The meeting title. Becomes the recording's auto-title.
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Attendee count, used by the `min_attendees` detection setting.
    /// Rooms and other resources are not counted.
    pub attendees: usize,
    /// Who is invited, as `meeting.md` stores them (SPEC §3.2): the display
    /// name, else the email address. Someone with neither is counted in
    /// [`Event::attendees`] but not named here.
    pub attendee_names: Vec<String>,
}

/// A source of calendar events.
///
/// Read-only by design — meet-ai never writes to anyone's calendar.
pub trait CalendarProvider {
    /// Human-readable provider name, for the settings screen.
    fn name(&self) -> &'static str;

    /// Events overlapping `[from, to)`, sorted by start time.
    ///
    /// `Ok(vec![])` means the calendar was read and is empty. A calendar that
    /// cannot be read is always an `Err` — never an empty list — so the UI can
    /// tell "no meetings today" apart from "can't read your calendar".
    fn list_events(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<Event>, Error>;
}

/// Everything that can go wrong reading a calendar.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// macOS has not granted calendar access: denied, restricted by a
    /// profile, write-only, or the prompt was never answered.
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
