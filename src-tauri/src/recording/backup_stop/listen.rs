//! The silence stop's loop (TUR-145): read both channels, ask the
//! [`SilenceTimer`], put the countdown card up or down.
//!
//! Everything the loop touches outside itself comes in through [`World`]
//! (the clock, the setting, whether this recording is still the live one,
//! the pause between reads) and [`Card`] (the countdown card), so it is
//! tested here with fake feeds, a fake clock and a fake card; the real ones
//! are in `backup_stop.rs`.
//!
//! The loop ends when the recording does: both channels' feeds disconnect
//! once the session stops (`audio::tee`), and [`World::still_recording`]
//! catches a newer recording or a stop that has not closed the feeds yet.

use std::sync::mpsc::TryRecvError;
use std::time::Instant;

use audio::tee::TeeFeed;

use super::silence::{SilenceTimer, Step};
use super::speech::SpeechGate;
use crate::detection::popup::CountdownEnd;

/// The countdown card, as the loop drives it.
pub trait Card {
    /// Put it up. `false` when it could not be shown.
    fn show(&mut self) -> bool;
    /// How it ended, or `None` while it is still up.
    fn poll(&mut self) -> Option<CountdownEnd>;
    /// Take it down quietly, if it is up.
    fn withdraw(&mut self);
}

/// The rest of the app, as the loop sees it.
pub trait World {
    fn now(&mut self) -> Instant;
    /// "Stop after 10 min of silence" is on.
    fn enabled(&mut self) -> bool;
    /// The recording this loop watches is still the one recording.
    fn still_recording(&mut self) -> bool;
    /// Wait before the next read.
    fn pause(&mut self);
}

/// How the loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The silence card said stop: Stop now, or the countdown ran out.
    Stop,
    /// The recording ended some other way; nothing to do.
    RecordingOver,
}

/// One channel's copy of the audio and its speech detector.
pub struct Channel {
    feed: Option<TeeFeed>,
    gate: SpeechGate,
}

impl Channel {
    pub fn new(feed: TeeFeed, gate: SpeechGate) -> Self {
        Self {
            feed: Some(feed),
            gate,
        }
    }

    /// Score everything queued so far. `true` when it held speech.
    fn drain(&mut self) -> bool {
        let mut heard = false;
        while let Some(feed) = &self.feed {
            match feed.try_recv() {
                Ok(frames) => heard |= self.gate.push(&frames),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.feed = None,
            }
        }
        heard
    }

    fn is_open(&self) -> bool {
        self.feed.is_some()
    }
}

/// Run until the recording should stop for silence, or is over.
pub fn listen(
    channels: &mut [Channel],
    timer: &mut SilenceTimer,
    world: &mut dyn World,
    card: &mut dyn Card,
) -> Ended {
    loop {
        let mut heard = false;
        for channel in channels.iter_mut() {
            heard |= channel.drain();
        }
        let over = !world.still_recording() || !channels.iter().any(Channel::is_open);
        if over {
            if timer.is_asking() {
                card.withdraw();
            }
            return Ended::RecordingOver;
        }

        let now = world.now();
        if heard && timer.heard(now) == Step::Withdraw {
            card.withdraw();
        }
        if timer.is_asking()
            && let Some(end) = card.poll()
            && timer.answered(now, end)
        {
            return Ended::Stop;
        }
        match timer.tick(now, world.enabled()) {
            Step::Ask => {
                if !card.show() {
                    // Never stop a recording nobody was asked about.
                    timer.answered(now, CountdownEnd::Closed);
                }
            }
            Step::Withdraw => card.withdraw(),
            Step::Wait => {}
        }
        world.pause();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use audio::tee::{Tee, tee};

    use super::*;
    use crate::recording::backup_stop::silence::SILENCE_LIMIT;
    use crate::recording::backup_stop::speech::{LoudIsSpeech, level};

    const TICK: Duration = Duration::from_secs(30);

    /// A fake world: the clock moves [`TICK`] per pause, and each pause can
    /// feed the channels what the script says for that moment.
    struct Script {
        start: Instant,
        now: Instant,
        enabled: bool,
        recording_until: Option<Instant>,
        /// Per pause: what the mic and the system channel hear next.
        steps: VecDeque<(Vec<i16>, Vec<i16>)>,
        mic: Option<Tee>,
        sys: Option<Tee>,
    }

    impl Script {
        fn minutes(&self) -> f64 {
            self.now.duration_since(self.start).as_secs_f64() / 60.0
        }
    }

    impl World for Script {
        fn now(&mut self) -> Instant {
            self.now
        }
        fn enabled(&mut self) -> bool {
            self.enabled
        }
        fn still_recording(&mut self) -> bool {
            self.recording_until.is_none_or(|end| self.now < end)
        }
        fn pause(&mut self) {
            self.now += TICK;
            match self.steps.pop_front() {
                Some((mic, sys)) => {
                    if let Some(tee) = &self.mic {
                        tee.offer(&mic);
                    }
                    if let Some(tee) = &self.sys {
                        tee.offer(&sys);
                    }
                }
                // The script ran out: the recording stops, its feeds close.
                None => {
                    self.mic = None;
                    self.sys = None;
                }
            }
        }
    }

    /// A fake card that answers `answers` in turn, after `polls_to_answer`
    /// polls each, and remembers what happened to it.
    #[derive(Default)]
    struct FakeCard {
        can_show: bool,
        answers: VecDeque<CountdownEnd>,
        polls_to_answer: usize,
        polled: usize,
        up: bool,
        shown: usize,
        withdrawn: usize,
    }

    impl Card for FakeCard {
        fn show(&mut self) -> bool {
            if self.can_show {
                self.up = true;
                self.shown += 1;
                self.polled = 0;
            }
            self.can_show
        }
        fn poll(&mut self) -> Option<CountdownEnd> {
            assert!(self.up, "polled a card that is not up");
            self.polled += 1;
            if self.polled < self.polls_to_answer {
                return None;
            }
            self.up = false;
            Some(self.answers.pop_front().unwrap_or(CountdownEnd::TimedOut))
        }
        fn withdraw(&mut self) {
            if self.up {
                self.up = false;
                self.withdrawn += 1;
            }
        }
    }

    /// Fake audio levels per [`TICK`]: half a second of each channel's level.
    fn quiet() -> (Vec<i16>, Vec<i16>) {
        (level(0.5, 0), level(0.5, 120))
    }
    fn mic_talks() -> (Vec<i16>, Vec<i16>) {
        (level(0.5, 9_000), level(0.5, 0))
    }
    fn others_talk() -> (Vec<i16>, Vec<i16>) {
        (level(0.5, 0), level(0.5, 9_000))
    }

    fn run(steps: Vec<(Vec<i16>, Vec<i16>)>, card: &mut FakeCard) -> (Ended, f64) {
        let (mic, mic_feed) = tee();
        let (sys, sys_feed) = tee();
        let start = Instant::now();
        let mut world = Script {
            start,
            now: start,
            enabled: true,
            recording_until: None,
            steps: steps.into(),
            mic: Some(mic),
            sys: Some(sys),
        };
        let mut channels = [
            Channel::new(mic_feed, SpeechGate::new(Box::new(LoudIsSpeech))),
            Channel::new(sys_feed, SpeechGate::new(Box::new(LoudIsSpeech))),
        ];
        let mut timer = SilenceTimer::new(start, SILENCE_LIMIT);
        let ended = listen(&mut channels, &mut timer, &mut world, card);
        (ended, world.minutes())
    }

    fn card() -> FakeCard {
        FakeCard {
            can_show: true,
            polls_to_answer: 1,
            ..FakeCard::default()
        }
    }

    #[test]
    fn ten_minutes_of_room_tone_asks_and_the_countdown_stops_it() {
        let mut card = card();
        let (ended, minutes) = run(vec![quiet(); 60], &mut card);
        assert_eq!(ended, Ended::Stop);
        assert_eq!(card.shown, 1);
        assert!((10.0..=11.0).contains(&minutes), "stopped at {minutes} min");
    }

    #[test]
    fn talking_on_either_side_keeps_the_card_away() {
        let mut steps = Vec::new();
        for minute in 0..40 {
            // Someone speaks every 8 minutes, alternating sides.
            for half in 0..2 {
                steps.push(match (minute % 8, half, minute % 16 < 8) {
                    (0, 0, true) => mic_talks(),
                    (0, 0, false) => others_talk(),
                    _ => quiet(),
                });
            }
        }
        let mut card = card();
        let (ended, _) = run(steps, &mut card);
        assert_eq!(ended, Ended::RecordingOver);
        assert_eq!(card.shown, 0);
    }

    #[test]
    fn keep_recording_restarts_the_ten_minutes() {
        let mut card = FakeCard {
            answers: [CountdownEnd::KeepRecording, CountdownEnd::StopNow].into(),
            ..card()
        };
        let (ended, minutes) = run(vec![quiet(); 60], &mut card);
        assert_eq!(ended, Ended::Stop);
        assert_eq!(
            card.shown, 2,
            "asked again ten minutes after Keep recording"
        );
        assert!((20.0..=21.5).contains(&minutes), "stopped at {minutes} min");
    }

    #[test]
    fn speech_during_the_countdown_withdraws_the_card_quietly() {
        let mut steps = vec![quiet(); 21];
        steps.push(others_talk());
        steps.extend(vec![quiet(); 4]);
        let mut card = FakeCard {
            // Never answers on its own: the speech has to take it down.
            polls_to_answer: usize::MAX,
            ..card()
        };
        let (ended, _) = run(steps, &mut card);
        assert_eq!(ended, Ended::RecordingOver);
        assert_eq!(card.shown, 1);
        assert_eq!(card.withdrawn, 1);
    }

    #[test]
    fn a_card_that_cannot_be_shown_never_stops_the_recording() {
        let mut card = FakeCard {
            can_show: false,
            ..card()
        };
        let (ended, _) = run(vec![quiet(); 60], &mut card);
        assert_eq!(ended, Ended::RecordingOver);
    }

    #[test]
    fn the_loop_ends_with_the_recording_and_takes_its_card_down() {
        let (mic, mic_feed) = tee();
        let start = Instant::now();
        let mut world = Script {
            start,
            now: start,
            enabled: true,
            recording_until: Some(start + SILENCE_LIMIT + TICK * 2),
            steps: vec![quiet(); 60].into(),
            mic: Some(mic),
            sys: None,
        };
        let mut channels = [Channel::new(
            mic_feed,
            SpeechGate::new(Box::new(LoudIsSpeech)),
        )];
        let mut timer = SilenceTimer::new(start, SILENCE_LIMIT);
        let mut card = FakeCard {
            polls_to_answer: usize::MAX,
            ..card()
        };
        let ended = listen(&mut channels, &mut timer, &mut world, &mut card);
        assert_eq!(ended, Ended::RecordingOver);
        assert_eq!((card.shown, card.withdrawn), (1, 1));
    }

    #[test]
    fn with_the_setting_off_it_never_asks() {
        let (mic, mic_feed) = tee();
        let start = Instant::now();
        let mut world = Script {
            start,
            now: start,
            enabled: false,
            recording_until: None,
            steps: vec![quiet(); 60].into(),
            mic: Some(mic),
            sys: None,
        };
        let mut channels = [Channel::new(
            mic_feed,
            SpeechGate::new(Box::new(LoudIsSpeech)),
        )];
        let mut timer = SilenceTimer::new(start, SILENCE_LIMIT);
        let mut card = card();
        let ended = listen(&mut channels, &mut timer, &mut world, &mut card);
        assert_eq!(ended, Ended::RecordingOver);
        assert_eq!(card.shown, 0);
    }
}
