//! The Calendar API v3 shapes [`super::GoogleProvider`] reads: one page of
//! `events.list` and the parts of an event meet-ai uses.
//!
//! Every field is optional with a default, and every enum has an `Unknown`
//! fallback, so a field Google adds or a value it starts sending never breaks
//! a read. Only the fields named in [`super::FIELDS`] are ever sent.

// Adapted from github.com/fastrepl/anarlog/crates/google-calendar/src/types.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
// Copyright (c) 2023-present Fastrepl, Inc. Trimmed to the fields we read;
// the specta/utoipa derives dropped.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventStatus {
    Confirmed,
    Tentative,
    Cancelled,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttendeeResponseStatus {
    NeedsAction,
    Declined,
    Tentative,
    Accepted,
    #[serde(other)]
    Unknown,
}

/// One event. With `singleEvents=true` this is one occurrence, with its own
/// `id`, and `recurring_event_id` names the series.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(default)]
    pub status: Option<EventStatus>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub start: Option<EventDateTime>,
    #[serde(default)]
    pub end: Option<EventDateTime>,
    #[serde(default)]
    pub recurring_event_id: Option<String>,
    #[serde(default, rename = "iCalUID")]
    pub ical_uid: Option<String>,
    #[serde(default)]
    pub attendees: Option<Vec<Attendee>>,
    /// The Google Meet link Google adds to a Meet event (TUR-77).
    #[serde(default)]
    pub hangout_link: Option<String>,
    /// Any conference on the event, Meet or an add-on like Zoom (TUR-77).
    #[serde(default)]
    pub conference_data: Option<ConferenceData>,
    /// Free text where an invite often pastes its call link (TUR-86).
    #[serde(default)]
    pub location: Option<String>,
    /// The event's notes, often HTML; the other place a link is pasted
    /// (TUR-86).
    #[serde(default)]
    pub description: Option<String>,
}

/// A start or end: `date_time` for a timed event, `date` alone for an
/// all-day one.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDateTime {
    #[serde(default)]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub date_time: Option<DateTime<FixedOffset>>,
    #[serde(default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    /// This attendee is the signed-in account.
    #[serde(default, rename = "self")]
    pub is_self: Option<bool>,
    /// A room or other resource, not a person.
    #[serde(default)]
    pub resource: Option<bool>,
    #[serde(default)]
    pub response_status: Option<AttendeeResponseStatus>,
}

/// One page of `events.list`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventsResponse {
    #[serde(default)]
    pub next_page_token: Option<String>,
    #[serde(default)]
    pub items: Vec<Event>,
}

// --- Ours, not from anarlog ---

/// Google's error body: `{"error": {"code": 403, "status": "PERMISSION_DENIED",
/// "errors": [{"reason": "accessNotConfigured"}], "message": "…"}}`. Only the
/// status and reason are read; the message can name the account.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ErrorResponse {
    #[serde(default)]
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ErrorBody {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub errors: Vec<ErrorItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ErrorItem {
    #[serde(default)]
    pub reason: Option<String>,
}

/// An event's conference: Meet, or an add-on such as Zoom (TUR-77). Only its
/// entry points are read.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConferenceData {
    #[serde(default)]
    pub entry_points: Vec<EntryPoint>,
}

/// One way into a conference: `video` (the link), `phone`, `sip` or `more`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPoint {
    #[serde(default)]
    pub entry_point_type: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
}
