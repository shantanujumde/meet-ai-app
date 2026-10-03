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

/// The online meeting's link: `calendar::Event::join_url` (TUR-77).
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
    /// TUR-77: its `join_url` becomes `calendar::Event::join_url`.
    #[serde(default)]
    pub online_meeting: Option<OnlineMeetingInfo>,
    #[serde(default)]
    pub response_status: Option<ResponseStatus>,
    /// Where a call link is often pasted when there is no online meeting
    /// (TUR-86).
    #[serde(default)]
    pub location: Option<Location>,
    /// The body's first 255 characters as plain text (TUR-86).
    #[serde(default)]
    pub body_preview: Option<String>,
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

/// An event's location; only the text shown for it is read (TUR-86).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    #[serde(default)]
    pub display_name: Option<String>,
}

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
