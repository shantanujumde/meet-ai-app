//! The countdown wiring with a fake card, a fake clock and fake mic readings.

use std::cell::{Cell, RefCell};

use audio::mic_users::MicApp;
use detect::Signal;

use super::*;
use crate::detection::notify::prompt_for;
use crate::recording::Phase;

/// A card that ends when the test says, counting its cancels.
#[derive(Default)]
struct FakeCard {
    /// Ends with this on the tick numbered by the key.
    ends: RefCell<Vec<(u32, CountdownEnd)>>,
    tick: Cell<u32>,
    cancels: Cell<u32>,
}

impl FakeCard {
    fn ending(at_tick: u32, end: CountdownEnd) -> Self {
        Self {
            ends: RefCell::new(vec![(at_tick, end)]),
            ..Self::default()
        }
    }
}

impl CountdownCard for FakeCard {
    fn try_end(&self) -> Option<CountdownEnd> {
        let tick = self.tick.get();
        let mut ends = self.ends.borrow_mut();
        let at = ends.iter().position(|(at, _)| *at <= tick)?;
        Some(ends.remove(at).1)
    }
    fn cancel(&self) {
        self.cancels.set(self.cancels.get() + 1);
    }
}

fn zoom() -> MicUser {
    MicUser::new("us.zoom.xos", "Zoom", MicKind::CallApp)
}

/// Rules with Zoom's countdown up at `start`; its id.
fn counting(start: Instant) -> (Watch, u32) {
    let watch = Watch::default();
    let id = watch.with(|rules| {
        rules.observe(&MicReading::Supported(vec![zoom()]), true, start);
        rules.observe(&MicReading::Supported(Vec::new()), true, start);
        match rules.observe(
            &MicReading::Supported(Vec::new()),
            true,
            start + detect::call_end::OFF_WAIT,
        ) {
            Step::Show { id, .. } => id,
            other => panic!("expected the countdown, got {other:?}"),
        }
    });
    (watch, id)
}

/// Drive `card` with a clock that moves one [`TICK`] per wait, and a
/// recording flag the test can flip on a given tick.
fn run(card: &FakeCard, watch: &Watch, id: u32, start: Instant, stopped_at: Option<u32>) -> bool {
    let now = Cell::new(start + detect::call_end::OFF_WAIT);
    drive(
        card,
        id,
        watch,
        || stopped_at.is_none_or(|at| card.tick.get() < at),
        || now.get(),
        || {
            card.tick.set(card.tick.get() + 1);
            now.set(now.get() + TICK);
            assert!(card.tick.get() < 1000, "the countdown never ended");
        },
    )
}

#[test]
fn stop_now_and_zero_stop_the_recording() {
    for end in [CountdownEnd::StopNow, CountdownEnd::TimedOut] {
        let start = Instant::now();
        let (watch, id) = counting(start);
        let card = FakeCard::ending(8, end);
        assert!(run(&card, &watch, id, start, None), "{end:?}");
        assert_eq!(card.tick.get(), 8);
        assert_eq!(watch.with(|rules| rules.counting()), None);
    }
}

#[test]
fn keep_recording_and_a_closed_card_never_stop() {
    for end in [CountdownEnd::KeepRecording, CountdownEnd::Closed] {
        let start = Instant::now();
        let (watch, id) = counting(start);
        let card = FakeCard::ending(3, end);
        assert!(!run(&card, &watch, id, start, None), "{end:?}");
        assert_eq!(watch.with(|rules| rules.counting()), None);
    }
}

#[test]
fn the_rules_stop_at_zero_when_the_card_never_says() {
    let start = Instant::now();
    let (watch, id) = counting(start);
    let card = FakeCard::default();
    assert!(run(&card, &watch, id, start, None));
    let ticks = (u64::from(detect::call_end::COUNTDOWN_SECONDS) * 1000
        + detect::call_end::TIMEOUT_GRACE.as_millis() as u64)
        / TICK.as_millis() as u64;
    assert_eq!(u64::from(card.tick.get()), ticks);
    // The card still up is closed.
    assert_eq!(card.cancels.get(), 1);
}

#[test]
fn stopping_by_hand_closes_the_card_without_stopping_again() {
    let start = Instant::now();
    let (watch, id) = counting(start);
    let card = FakeCard::default();
    assert!(!run(&card, &watch, id, start, Some(4)));
    assert_eq!(card.tick.get(), 4);
    assert_eq!(card.cancels.get(), 1);
}

#[test]
fn back_on_the_mic_closes_the_card_and_keeps_recording() {
    let start = Instant::now();
    let (watch, id) = counting(start);
    // The loop reads Zoom back on the mic while the countdown runs.
    let step = watch.with(|rules| {
        rules.observe(
            &MicReading::Supported(vec![zoom()]),
            true,
            start + Duration::from_secs(7),
        )
    });
    assert_eq!(step, Step::Cancel { id });
    let card = FakeCard::default();
    assert!(!run(&card, &watch, id, start, None));
    assert_eq!(card.tick.get(), 1, "seen on the next tick");
    assert_eq!(card.cancels.get(), 1);
}

fn mic_app(id: &str, name: &str, kind: AppKind) -> MicApp {
    MicApp {
        pid: 1,
        id: id.to_string(),
        name: name.to_string(),
        kind,
        playing: true,
    }
}

#[test]
fn the_reading_keeps_call_apps_browsers_and_others_but_never_system_tools() {
    let users = MicUsers::Supported(vec![
        mic_app("us.zoom.xos", "Zoom", AppKind::CallApp),
        mic_app("com.google.Chrome", "Google Chrome", AppKind::Browser),
        mic_app("com.apple.VoiceMemos", "Voice Memos", AppKind::Other),
        mic_app("com.krisp.krispMac", "Krisp", AppKind::IgnoredSystem),
    ]);
    assert_eq!(
        reading(users),
        MicReading::Supported(vec![
            MicUser::new("us.zoom.xos", "Zoom", MicKind::CallApp),
            MicUser::new("com.google.Chrome", "Google Chrome", MicKind::Browser),
            MicUser::new("com.apple.VoiceMemos", "Voice Memos", MicKind::Other),
        ])
    );
    assert_eq!(reading(MicUsers::NotSupported), MicReading::NotSupported);
}

#[test]
fn every_card_end_has_its_answer() {
    assert_eq!(answer_for(CountdownEnd::StopNow), Answer::StopNow);
    assert_eq!(
        answer_for(CountdownEnd::KeepRecording),
        Answer::KeepRecording
    );
    assert_eq!(answer_for(CountdownEnd::TimedOut), Answer::TimedOut);
    assert_eq!(answer_for(CountdownEnd::Closed), Answer::Closed);
    for end in [CountdownEnd::StopNow, CountdownEnd::TimedOut] {
        assert!(end.stops());
    }
}

fn shown(id: u32, card: Card) -> PopupPrompt {
    PopupPrompt {
        id,
        card,
        closes_at_ms: 0,
    }
}

fn zoom_prompt() -> Card {
    let signal = Signal::Process {
        process: detect::processes::name_of("Zoom").to_string(),
    };
    prompt_for(Phase::Idle, &signal).expect("idle").into()
}

#[test]
fn record_on_a_call_prompt_names_its_app() {
    let card = shown(4, zoom_prompt());
    assert_eq!(
        prompted_app(Some(&card), 4, PopupAnswer::Record),
        Some("Zoom".to_string())
    );
    // Any other answer, a stale id or no card: nothing.
    for answer in [
        PopupAnswer::Dismiss,
        PopupAnswer::NeverFor,
        PopupAnswer::Join,
        PopupAnswer::OpenBrief,
    ] {
        assert_eq!(prompted_app(Some(&card), 4, answer), None, "{answer:?}");
    }
    assert_eq!(prompted_app(Some(&card), 5, PopupAnswer::Record), None);
    assert_eq!(prompted_app(None, 4, PopupAnswer::Record), None);
}

#[test]
fn audio_activity_a_reminder_or_a_countdown_names_no_app() {
    let audio = prompt_for(Phase::Idle, &Signal::AudioActivity).expect("idle");
    assert_eq!(
        prompted_app(Some(&shown(1, audio.into())), 1, PopupAnswer::Record),
        None
    );
    let countdown = Card::Countdown {
        line: "Zoom call ended".to_string(),
        seconds: 10,
    };
    assert_eq!(
        prompted_app(Some(&shown(2, countdown)), 2, PopupAnswer::Record),
        None
    );
}

#[test]
fn the_followed_app_from_a_prompt_wins_over_the_first_seen() {
    let watch = Watch::default();
    let start = Instant::now();
    watch.with(|rules| rules.started_from_prompt("Zoom", start));
    let chrome = MicUser::new("com.google.Chrome", "Google Chrome", MicKind::Browser);
    watch.with(|rules| rules.observe(&MicReading::Supported(vec![chrome, zoom()]), true, start));
    assert_eq!(watch.with(|rules| rules.tracked().cloned()), Some(zoom()));
}
