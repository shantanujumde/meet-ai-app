//! Microsoft Graph's event shapes, as `GET /me/calendarView` returns them.
//!
//! Only the fields the provider asks for in `$select` (see
//! [`super::SELECT`]) and reads. Every field is optional or defaulted: Graph
//! leaves a field out rather than sending `null` in places, and a missing
//! field must never fail a whole day's read. Unknown enum values fall back to
//! `Unknown`, so a new Graph value does not either.

use serde::Deserialize;

// Adapted from github.com/fastrepl/anarlog/crates/outlook-calendar/src/types.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
// Trimmed to the fields we select; specta/utoipa derives and Serialize dropped.

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseType {
    None,
    Organizer,
    TentativelyAccepted,
    Accepted,
    Declined,
    NotResponded,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttendeeType {
    Required,
    Optional,
    Resource,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeTimeZone {
    pub date_time: String,
    #[serde(default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseStatus {
    #[serde(default)]
    pub response: Option<ResponseType>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default, rename = "type")]
    pub type_: Option<AttendeeType>,
    #[serde(default)]
    pub status: Option<ResponseStatus>,
    #[serde(default)]
    pub email_address: Option<EmailAddress>,
}

/// Parsed but not stored yet: `calendar::Event` has no `join_url` until
/// TUR-77 adds one; then `join_url` here fills it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineMeetingInfo {
    #[serde(default)]
    pub join_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    #[serde(default, rename = "iCalUId")]
    pub ical_uid: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub start: Option<DateTimeTimeZone>,
    #[serde(default)]
    pub end: Option<DateTimeTimeZone>,
    #[serde(default)]
    pub attendees: Option<Vec<Attendee>>,
    #[serde(default)]
    pub is_all_day: Option<bool>,
    #[serde(default)]
    pub is_cancelled: Option<bool>,
    /// TUR-77: `join_url` goes onto `calendar::Event` once it has the field.
    #[serde(default)]
    pub online_meeting: Option<OnlineMeetingInfo>,
    #[serde(default)]
    pub response_status: Option<ResponseStatus>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventsResponse {
    #[serde(default, rename = "@odata.nextLink")]
    pub odata_next_link: Option<String>,
    #[serde(default)]
    pub value: Vec<Event>,
}

// End of the adapted code.

/// Graph's error body: `{ "error": { "code": "...", "message": "..." } }`.
/// Only the code is logged; the message can name the account.
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorBody {
    #[serde(default)]
    pub code: Option<String>,
}
