//! One prompt per call, when the calendar and a running app both notice it
//! (TUR-30).
//!
//! The reminder fires a minute before an invite starts; Zoom opening, Slack
//! with a call signal, or audio activity usually follow within a minute or
//! two. Asking twice for the same call is nagging, so any two signals within
//! [`MERGE_WINDOW`] of each other become one prompt:
//!
//! - **Reminder first.** A later process or audio-activity signal is dropped
//!   while the reminded event has not ended: the reminder already asks, and it
//!   names the meeting and links its brief.
//! - **App first.** A reminder soon after only updates the prompt already on
//!   screen (the window swaps in the meeting's title and its brief), with no
//!   second system notification. If that prompt was dismissed, nothing comes
//!   back.
//!
//! A call prompt naming the app on the mic (TUR-143, "WhatsApp call
//! detected") is a process signal here: it takes the same [`Merger::other`].
//!
//! Pure state over wall-clock times passed in, so every rule is a unit test.

use chrono::{DateTime, Duration, Utc};

/// How close two signals must be to count as the same call.
pub const MERGE_WINDOW: Duration = Duration::minutes(3);

/// How a prompt reaches the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// A system notification and a new banner.
    New,
    /// Replace the banner on screen, if there is one; no notification.
    UpdateOnly,
    /// Already asked about this call; say nothing.
    Drop,
}

/// The last prompts asked, kept to merge the next one into.
#[derive(Debug, Default)]
pub struct Merger {
    /// When the last reminder asked, and when its event ends.
    reminder: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// When the last process or audio-activity prompt asked.
    other: Option<DateTime<Utc>>,
}

impl Merger {
    /// A reminder for an event ending at `ends` is about to ask, at `now`.
    pub fn reminder(&mut self, now: DateTime<Utc>, ends: DateTime<Utc>) -> Delivery {
        let after_other = self.other.is_some_and(|at| within(at, now));
        self.reminder = Some((now, ends));
        if after_other {
            Delivery::UpdateOnly
        } else {
            Delivery::New
        }
    }

    /// A process or audio-activity signal is about to ask, at `now`.
    pub fn other(&mut self, now: DateTime<Utc>) -> Delivery {
        if let Some((at, ends)) = self.reminder
            && within(at, now)
            && now < ends
        {
            return Delivery::Drop;
        }
        self.other = Some(now);
        Delivery::New
    }
}

/// `now` is at or after `at`, by at most [`MERGE_WINDOW`].
fn within(at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now >= at && now - at <= MERGE_WINDOW
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, h, m, s).unwrap()
    }

    #[test]
    fn alone_each_signal_asks_as_new() {
        assert_eq!(
            Merger::default().reminder(at(9, 59, 0), at(10, 30, 0)),
            Delivery::New
        );
        assert_eq!(Merger::default().other(at(9, 59, 0)), Delivery::New);
    }

    #[test]
    fn zoom_opening_after_the_reminder_is_the_same_prompt() {
        let mut merger = Merger::default();
        assert_eq!(merger.reminder(at(9, 59, 0), at(10, 30, 0)), Delivery::New);
        assert_eq!(merger.other(at(10, 0, 30)), Delivery::Drop);
        assert_eq!(merger.other(at(10, 2, 0)), Delivery::Drop);
    }

    #[test]
    fn an_app_past_the_window_asks_again() {
        let mut merger = Merger::default();
        merger.reminder(at(9, 59, 0), at(10, 30, 0));
        assert_eq!(merger.other(at(10, 2, 1)), Delivery::New);
    }

    #[test]
    fn an_app_after_a_short_event_has_ended_asks() {
        let mut merger = Merger::default();
        merger.reminder(at(9, 59, 0), at(10, 1, 0));
        assert_eq!(merger.other(at(10, 1, 0)), Delivery::New);
    }

    #[test]
    fn a_reminder_after_zoom_only_updates_the_open_prompt() {
        let mut merger = Merger::default();
        assert_eq!(merger.other(at(9, 57, 30)), Delivery::New);
        assert_eq!(
            merger.reminder(at(9, 59, 0), at(10, 30, 0)),
            Delivery::UpdateOnly
        );
        // And the call's audio a minute later stays quiet.
        assert_eq!(merger.other(at(10, 0, 0)), Delivery::Drop);
    }

    #[test]
    fn a_reminder_long_after_an_app_prompt_is_new() {
        let mut merger = Merger::default();
        merger.other(at(9, 0, 0));
        assert_eq!(merger.reminder(at(9, 59, 0), at(10, 30, 0)), Delivery::New);
    }

    #[test]
    fn a_clock_that_went_back_merges_nothing() {
        let mut merger = Merger::default();
        merger.reminder(at(9, 59, 0), at(10, 30, 0));
        assert_eq!(merger.other(at(9, 58, 0)), Delivery::New);
    }
}
