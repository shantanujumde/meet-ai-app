use std::cell::RefCell;

use super::*;
use crate::chime::samples;

/// The rate the capture path writes, so the rate the check runs at.
const RATE: u32 = 16_000;

/// How much audio each `pull` hands over. Real taps deliver ~10 ms buffers.
const CHUNK_MILLIS: u32 = 10;

/// The measured settle time from TUR-4 (see `ONSET_TIMEOUT_MILLIS`).
const MEASURED_SETTLE_MILLIS: u32 = 1070;

fn ms(millis: u32) -> usize {
    samples_in(millis, RATE)
}

/// Stands in for what a freshly-created tap delivers before it settles:
/// loud (peak up to 0.41, like the measured artefact), not silence, and not
/// our chime. A mix of three low, unrelated tones, deterministic so the test
/// that proves the detector rejects it proves it for every run.
///
/// Not white noise on purpose: broadband noise this loud puts ~0.01 into
/// both chime bins — twice `DETECT_FLOOR` — and somewhere in a few seconds
/// of it the two bins happen to line up with enough contrast to pass. That
/// would let the fake tap "hear" a chime that never arrived and hide the
/// very loss these tests are about.
fn unsettled(index: usize) -> f32 {
    let seconds = index as f32 / RATE as f32;
    let tone = |hz: f32, phase: f32| (std::f32::consts::TAU * hz * seconds + phase).sin();
    0.41 * (0.5 * tone(170.0, 0.0) + 0.3 * tone(310.0, 1.0) + 0.2 * tone(455.0, 2.0))
}

/// A fake process tap. Time is the number of samples delivered so far.
///
/// `play()` records the current tap time; the chime then shows up in the
/// stream `latency` later — but only the parts that arrive after the tap has
/// settled. Before `settle_at` the tap delivers [`unsettled`] noise whatever
/// is playing, which is how a play can be lost.
struct Tap {
    settle_at: usize,
    latency: usize,
    /// Every sample is a bit-exact zero, forever (the measured denial).
    denied: bool,
    /// Stop delivering (`None`) once this much has been pulled.
    stops_at: Option<usize>,
    position: usize,
    plays: Vec<usize>,
    pulls: usize,
    chime: Vec<f32>,
}

impl Tap {
    fn granted(settle_millis: u32, latency_millis: u32) -> Self {
        Self {
            settle_at: ms(settle_millis),
            latency: ms(latency_millis),
            denied: false,
            stops_at: None,
            position: 0,
            plays: Vec::new(),
            pulls: 0,
            chime: samples(RATE),
        }
    }

    fn denied() -> Self {
        Self {
            denied: true,
            ..Self::granted(MEASURED_SETTLE_MILLIS, 30)
        }
    }

    fn play(&mut self) {
        self.plays.push(self.position);
    }

    fn pull(&mut self) -> Option<Vec<f32>> {
        self.pulls += 1;
        if self.stops_at.is_some_and(|stop| self.position >= stop) {
            return None;
        }
        let start = self.position;
        self.position += ms(CHUNK_MILLIS);
        Some((start..self.position).map(|t| self.sample(t)).collect())
    }

    fn sample(&self, t: usize) -> f32 {
        if self.denied {
            return 0.0;
        }
        if t < self.settle_at {
            return unsettled(t);
        }
        self.plays
            .iter()
            .filter_map(|played| t.checked_sub(played + self.latency))
            .filter_map(|offset| self.chime.get(offset))
            .sum()
    }
}

fn run(tap: Tap) -> (Attempt, Tap) {
    let tap = RefCell::new(tap);
    let attempt = play_until_heard(RATE, || tap.borrow_mut().play(), || tap.borrow_mut().pull());
    (attempt, tap.into_inner())
}

#[test]
fn the_unsettled_noise_is_loud_and_is_not_mistaken_for_the_chime() {
    // The fake tap is only a fair test if its pre-settle noise is the hard
    // case: loud, not silent, and still not a pass on its own.
    let noise: Vec<f32> = (0..ms(3_000)).map(unsettled).collect();
    let peak = noise.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    assert!((0.3..=0.41).contains(&peak), "peak {peak}");
    let reading = heard(&noise, RATE);
    assert!(!reading.present, "{reading:?}");
}

#[test]
fn a_normal_start_plays_once_and_stops_listening_right_after_the_chime() {
    let (attempt, tap) = run(Tap::granted(MEASURED_SETTLE_MILLIS, 30));

    assert!(attempt.reading.present, "{attempt:?}");
    assert_eq!(attempt.plays, 1);
    assert_eq!(
        tap.plays,
        vec![ms(SETTLE_MILLIS)],
        "plays once, after settle"
    );
    // Heard as soon as the chime has fully arrived — not at the end of the
    // listen window, and nowhere near the worst case.
    let chime_ends = SETTLE_MILLIS + 30 + duration_millis();
    assert!(
        attempt.captured <= ms(chime_ends + 2 * CHUNK_MILLIS),
        "captured {} samples, chime ended at {}",
        attempt.captured,
        ms(chime_ends)
    );
    assert!(chime_ends + 2 * CHUNK_MILLIS < SETTLE_MILLIS + duration_millis() + LISTEN_TAIL_MILLIS);
}

#[test]
fn the_measured_settle_time_is_not_a_false_denied() {
    // TUR-4 measured the tap clearing its threshold 1.07 s after starting.
    // The single play has to land after that on its own, with no retry.
    for latency in [0, 10, 30, 80] {
        let (attempt, _) = run(Tap::granted(MEASURED_SETTLE_MILLIS, latency));
        assert!(attempt.reading.present, "latency {latency} ms: {attempt:?}");
        assert_eq!(attempt.plays, 1, "latency {latency} ms");
    }
}

#[test]
fn a_denied_tap_gets_exactly_two_plays_then_a_failed_reading() {
    let (attempt, tap) = run(Tap::denied());

    assert!(!attempt.reading.present, "{attempt:?}");
    assert_eq!(attempt.plays, MAX_PLAYS);
    assert_eq!(tap.plays.len(), 2);
    assert!(
        tap.plays[1] >= ms(ONSET_TIMEOUT_MILLIS),
        "the retry must land past ONSET_TIMEOUT_MILLIS, got {}",
        tap.plays[1]
    );
    assert!(
        attempt.captured >= ms(worst_case_millis())
            && attempt.captured <= ms(worst_case_millis() + CHUNK_MILLIS),
        "captured {} samples, worst case {}",
        attempt.captured,
        ms(worst_case_millis())
    );
    // Zeros read as exactly zero: "heard nothing", not "heard noise".
    for note in &attempt.reading.notes {
        assert_eq!(note.magnitude, 0.0, "{note:?}");
    }
}

#[test]
fn a_tap_that_settles_late_is_rescued_by_the_retry() {
    // Settles at 1.6 s: the first chime (1.2 s + latency) lands inside the
    // unsettled noise and is lost; the retry lands after and is heard.
    let (attempt, tap) = run(Tap::granted(1_600, 30));

    assert!(attempt.reading.present, "{attempt:?}");
    assert_eq!(attempt.plays, 2);
    assert!(tap.plays[1] >= ms(ONSET_TIMEOUT_MILLIS));
}

#[test]
fn once_heard_nothing_more_is_pulled_or_played() {
    let (attempt, tap) = run(Tap::granted(MEASURED_SETTLE_MILLIS, 30));

    assert!(attempt.reading.present, "{attempt:?}");
    assert_eq!(tap.plays.len(), 1, "no second play after a pass");
    // Every pull delivered a chunk that was counted: the function did not
    // pull again after the pass and throw the chunk away.
    assert_eq!(tap.pulls * ms(CHUNK_MILLIS), attempt.captured);
    assert_eq!(tap.position, attempt.captured);
}

#[test]
fn a_tap_that_delivers_nothing_plays_nothing() {
    let mut tap = Tap::granted(MEASURED_SETTLE_MILLIS, 30);
    tap.stops_at = Some(0);
    let (attempt, tap) = run(tap);

    assert_eq!(attempt.plays, 0);
    assert_eq!(attempt.captured, 0);
    assert!(!attempt.reading.present);
    assert!(attempt.reading.notes.is_empty());
    assert_eq!(tap.pulls, 1);
}

#[test]
fn a_tap_that_stops_mid_listen_returns_what_it_has() {
    // Stops 100 ms after the play: the chime has only partly arrived.
    let mut tap = Tap::granted(MEASURED_SETTLE_MILLIS, 30);
    tap.stops_at = Some(ms(SETTLE_MILLIS + 100));
    let (attempt, _) = run(tap);

    assert_eq!(attempt.plays, 1);
    assert_eq!(attempt.captured, ms(SETTLE_MILLIS + 100));
    assert!(!attempt.reading.present, "{attempt:?}");
}

#[test]
fn a_tap_that_stops_before_settling_never_plays() {
    let mut tap = Tap::granted(MEASURED_SETTLE_MILLIS, 30);
    tap.stops_at = Some(ms(500));
    let (attempt, _) = run(tap);

    assert_eq!(attempt.plays, 0);
    assert_eq!(attempt.captured, ms(500));
    assert!(!attempt.reading.present, "{attempt:?}");
}

#[test]
fn bluetooth_output_latency_is_still_heard_on_the_first_play() {
    for latency in [200, 400, 550] {
        let (attempt, _) = run(Tap::granted(MEASURED_SETTLE_MILLIS, latency));
        assert!(attempt.reading.present, "latency {latency} ms: {attempt:?}");
        assert_eq!(attempt.plays, 1, "latency {latency} ms");
    }
}

#[test]
fn the_worst_case_covers_both_plays_and_their_listen_windows() {
    let listen = duration_millis() + LISTEN_TAIL_MILLIS;
    const { assert!(SETTLE_MILLIS > MEASURED_SETTLE_MILLIS) };
    assert!(worst_case_millis() >= ONSET_TIMEOUT_MILLIS + listen);
    assert!(worst_case_millis() >= SETTLE_MILLIS + 2 * listen);
}

#[test]
fn the_attempt_says_why_it_ended_and_how_loud_the_audio_after_the_play_was() {
    let (heard_attempt, _) = run(Tap::granted(MEASURED_SETTLE_MILLIS, 30));
    assert_eq!(heard_attempt.finish, Finish::Heard);
    assert!(heard_attempt.peak > 0.0 && heard_attempt.rms > 0.0);

    let (denied, _) = run(Tap::denied());
    assert_eq!(denied.finish, Finish::Window);
    assert_eq!((denied.peak, denied.rms), (0.0, 0.0));
    assert!(denied.listened >= ms(duration_millis() + LISTEN_TAIL_MILLIS));

    let mut tap = Tap::granted(MEASURED_SETTLE_MILLIS, 30);
    tap.stops_at = Some(ms(SETTLE_MILLIS + 100));
    let (cut, _) = run(tap);
    assert_eq!(cut.finish, Finish::Pulled);
    assert_eq!(cut.listened, ms(100));
}
