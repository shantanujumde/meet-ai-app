//! Frontmatter keys in `meeting.md` that more than one writer sets (SPEC §3.2).
//!
//! ```yaml
//! ---
//! id: 2026-09-01-1430
//! title: Platform Standup                 # TITLE
//! title_source: calendar                  # TITLE_SOURCE
//! title_hash: 06d2abde48536429            # TITLE_HASH
//! attendees: [Shantanu, Priya, Dev]       # ATTENDEES
//! calendar_event_id: "ABC123"             # CALENDAR_EVENT_ID
//! ---
//! ```
//!
//! The recorder fills the title, attendees and event id from the calendar
//! event the recording belongs to (TUR-29). The notes run may then replace the
//! title with the agent's own suggestion, and the user may rename the meeting
//! at any time (TUR-103); `title_source` says which of the three wrote the
//! title, so a name the user gave is never replaced. `title_hash` lets a
//! title edited by hand in `meeting.md` count as the user's too (TUR-107).
//! Naming a meeting never renames its folder: the folder id follows
//! [`crate::layout`] and the time the recording started, nothing else.

/// The meeting's title, shown in the meeting list.
pub const TITLE: &str = "title";

/// Who was invited, as a list of names: each attendee's display name, else
/// their email address.
pub const ATTENDEES: &str = "attendees";

/// The calendar's id for the event this meeting was recorded during, written
/// as a string even when it looks like a number.
pub const CALENDAR_EVENT_ID: &str = "calendar_event_id";

/// Who wrote [`TITLE`]: `calendar`, `agent` or `user` (TUR-103). Missing on a
/// title nobody set through the app, which counts as typed by hand.
pub const TITLE_SOURCE: &str = "title_source";

/// A short hash of the title the calendar or the agent last wrote, next to
/// [`TITLE_SOURCE`] (TUR-107). A title that no longer matches it was edited by
/// hand, so it counts as the user's. Not written with a user's title.
pub const TITLE_HASH: &str = "title_hash";
