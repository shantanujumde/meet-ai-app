//! The 10-minute silence timer (TUR-145), with no audio, clock or window.
//!
//! A recording that nobody stops can run for hours: a call app that keeps
//! the mic after hang-up, or someone who walked away. So when no one has
//! spoken on either side for [`SILENCE_LIMIT`], the recorder asks, with the
//! countdown card: "No one has spoken for 10 minutes", Stop now and Keep
//! recording. This is the decision half: it is told when speech was heard and
//! what the time is, and says when to put the card up, when to take it down
//! again, and whether the answer stops the recording.
//!
//! * Speech on either channel restarts the timer, and takes a card that is
//!   already up down again quietly: someone is talking, so the call is not
//!   over.
//! * **Keep recording** restarts the timer, and so does a card that closed
//!   without an answer (replaced by another card, or never shown): a card
//!   nobody saw never stops a recording.
//! * **Stop now** and the countdown reaching zero stop it.
//! * With the setting off nothing is asked, and the timer starts fresh from
//!   the moment it is turned back on.

use std::time::{Duration, Instant};

use crate::detection::popup::CountdownEnd;

/// How long with no speech on either side before the recorder asks.
pub const SILENCE_LIMIT: Duration = Duration::from_secs(10 * 60);

/// What the caller should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing changes.
    Wait,
    /// Put the countdown card up.
    Ask,
    /// Take the card that is up down again, quietly.
    Withdraw,
}

/// Silence since when, and whether the card is up.
#[derive(Debug, Clone)]
pub struct SilenceTimer {
    limit: Duration,
    quiet_since: Instant,
    asking: bool,
}

impl SilenceTimer {
    /// A timer that starts counting at `now` and asks after `limit`.
    pub fn new(now: Instant, limit: Duration) -> Self {
        Self {
            limit,
            quiet_since: now,
            asking: false,
        }
    }

    /// Whether the card is up.
    pub fn is_asking(&self) -> bool {
        self.asking
    }

    /// Someone spoke at `now`: start again, and withdraw the card if it is up.
    pub fn heard(&mut self, now: Instant) -> Step {
        self.restart(now);
        if std::mem::take(&mut self.asking) {
            Step::Withdraw
        } else {
            Step::Wait
        }
    }

    /// The time is `now` and the setting is `enabled`: ask once the silence
    /// has lasted the limit. Turning the setting off withdraws a card that is
    /// up, and holds the timer at zero until it is on again.
    pub fn tick(&mut self, now: Instant, enabled: bool) -> Step {
        if !enabled {
            self.restart(now);
            return if std::mem::take(&mut self.asking) {
                Step::Withdraw
            } else {
                Step::Wait
            };
        }
        if !self.asking && now.saturating_duration_since(self.quiet_since) >= self.limit {
            self.asking = true;
            return Step::Ask;
        }
        Step::Wait
    }

    /// The card ended at `now` with `end`. `true` means stop the recording;
    /// any other ending starts the ten minutes again.
    pub fn answered(&mut self, now: Instant, end: CountdownEnd) -> bool {
        self.asking = false;
        if end.stops() {
            return true;
        }
        self.restart(now);
        false
    }

    fn restart(&mut self, now: Instant) {
        self.quiet_since = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    /// A fake clock: `at(minutes)` after a fixed start.
    struct Clock(Instant);

    impl Clock {
        fn at(&self, minutes: u64) -> Instant {
            self.0 + MINUTE * u32::try_from(minutes).unwrap()
        }
    }

    fn start() -> (Clock, SilenceTimer) {
        let clock = Clock(Instant::now());
        let timer = SilenceTimer::new(clock.at(0), SILENCE_LIMIT);
        (clock, timer)
    }

    #[test]
    fn ten_minutes_of_silence_asks_once() {
        let (clock, mut timer) = start();
        for minute in 0..10 {
            assert_eq!(timer.tick(clock.at(minute), true), Step::Wait, "{minute}");
        }
        assert_eq!(timer.tick(clock.at(10), true), Step::Ask);
        assert!(timer.is_asking());
        assert_eq!(timer.tick(clock.at(11), true), Step::Wait, "asks only once");
    }

    #[test]
    fn speech_restarts_the_ten_minutes() {
        let (clock, mut timer) = start();
        assert_eq!(timer.heard(clock.at(9)), Step::Wait);
        assert_eq!(timer.tick(clock.at(18), true), Step::Wait);
        assert_eq!(timer.tick(clock.at(19), true), Step::Ask);
    }

    #[test]
    fn speech_while_the_card_is_up_withdraws_it_and_starts_again() {
        let (clock, mut timer) = start();
        assert_eq!(timer.tick(clock.at(10), true), Step::Ask);
        assert_eq!(timer.heard(clock.at(10)), Step::Withdraw);
        assert!(!timer.is_asking());
        // The card's own `Closed` ending arrives after; it stops nothing.
        assert!(!timer.answered(clock.at(10), CountdownEnd::Closed));
        assert_eq!(timer.tick(clock.at(19), true), Step::Wait);
        assert_eq!(timer.tick(clock.at(20), true), Step::Ask);
    }

    #[test]
    fn keep_recording_restarts_the_timer() {
        let (clock, mut timer) = start();
        assert_eq!(timer.tick(clock.at(10), true), Step::Ask);
        assert!(!timer.answered(clock.at(10), CountdownEnd::KeepRecording));
        assert_eq!(timer.tick(clock.at(19), true), Step::Wait);
        assert_eq!(timer.tick(clock.at(20), true), Step::Ask);
    }

    #[test]
    fn only_stop_now_and_the_countdown_running_out_stop() {
        for (end, stops) in [
            (CountdownEnd::StopNow, true),
            (CountdownEnd::TimedOut, true),
            (CountdownEnd::KeepRecording, false),
            (CountdownEnd::Closed, false),
        ] {
            let (clock, mut timer) = start();
            assert_eq!(timer.tick(clock.at(10), true), Step::Ask);
            assert_eq!(timer.answered(clock.at(10), end), stops, "{end:?}");
            assert!(!timer.is_asking());
        }
    }

    #[test]
    fn with_the_setting_off_nothing_is_asked_and_on_again_starts_fresh() {
        let (clock, mut timer) = start();
        for minute in 0..=30 {
            assert_eq!(timer.tick(clock.at(minute), false), Step::Wait);
        }
        assert_eq!(timer.tick(clock.at(39), true), Step::Wait);
        assert_eq!(timer.tick(clock.at(40), true), Step::Ask);
    }

    #[test]
    fn turning_the_setting_off_withdraws_the_card() {
        let (clock, mut timer) = start();
        assert_eq!(timer.tick(clock.at(10), true), Step::Ask);
        assert_eq!(timer.tick(clock.at(10), false), Step::Withdraw);
        assert!(!timer.is_asking());
    }

    #[test]
    fn a_clock_that_seems_to_go_back_never_asks_early() {
        let (clock, mut timer) = start();
        timer.heard(clock.at(5));
        assert_eq!(timer.tick(clock.at(1), true), Step::Wait);
    }
}
