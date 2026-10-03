//! Frontmatter keys in `meeting.md` that more than one writer sets (SPEC §3.2).
//!
//! ```yaml
//! ---
//! id: 2026-09-01-1430
//! title: Platform Standup                 # TITLE
//! attendees: [Shantanu, Priya, Dev]       # ATTENDEES
//! calendar_event_id: "ABC123"             # CALENDAR_EVENT_ID
//! ---
//! ```
//!
//! The recorder fills these three from the calendar event the recording
//! belongs to (TUR-29); the notes run and the user keep what is there. Naming
//! a meeting never renames its folder: the folder id follows [`crate::layout`]
//! and the time the recording started, nothing else.

/// The meeting's title, shown in the meeting list.
pub const TITLE: &str = "title";

/// Who was invited, as a list of names: each attendee's display name, else
/// their email address.
pub const ATTENDEES: &str = "attendees";

/// The calendar's id for the event this meeting was recorded during, written
/// as a string even when it looks like a number.
pub const CALENDAR_EVENT_ID: &str = "calendar_event_id";
