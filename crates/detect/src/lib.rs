//! Meeting detection for meet-ai.
//!
//! Phase 5a territory (SPEC §5). Nothing is implemented yet.
//!
//! L15: detection never starts a recording on its own. It notifies, and the user
//! confirms. SPEC §2.3 also rules browser-based meetings out of v1 — Google Meet
//! in a tab is not detectable without a browser extension, and calendar plus
//! audio activity covers it.

#![forbid(unsafe_op_in_unsafe_fn)]

/// Process names that mean a meeting app is running (SPEC §2.3).
pub const MEETING_PROCESSES: &[&str] = &["zoom.us", "Microsoft Teams", "Webex", "Slack", "Discord"];

/// Why meet-ai thinks a meeting is happening.
///
/// Carried through to the notification copy, so the user is told *why* they are
/// being asked rather than just being asked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Signal {
    /// A calendar event with enough attendees is starting.
    Calendar { title: String, attendees: usize },
    /// A known meeting application is running.
    Process { process: String },
    /// Something is playing audio through the default output device.
    AudioActivity,
}

/// Everything that can go wrong while detecting.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The process list could not be read.
    #[error("could not read the list of running applications")]
    ProcessList(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meeting_process_list_is_not_empty() {
        assert!(MEETING_PROCESSES.contains(&"zoom.us"));
    }
}
