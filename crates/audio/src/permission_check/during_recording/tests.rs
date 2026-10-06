//! The check that runs during a recording, driven headless: a fake tap
//! behind a real fan-out tee, on a fake clock, so the verdict, the plays and
//! the transcript's hush are all checked without a tap, an output device or
//! real time passing.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::time::Duration;

use super::*;
use crate::tee::{Tee, TeeFeed, tee};

const RATE: u32 = verdict::RATE;

fn samples(millis: u64) -> usize {
    (millis * u64::from(RATE) / 1000) as usize
}

struct FakeClock(Cell<Duration>);

impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        self.0.get()
    }
}

/// The recording's system source, faked: every `recv` the check makes
/// delivers the next 10 ms of "captured" audio into the recording's
/// fan-out tee, the way the source's worker thread would, and the check
/// reads it back from its own copy.
struct FakeTap {
    /// Where the source offers: the transcript's hushed tee, fanned out to
    /// the check's.
    tee: Tee,
    check_feed: TeeFeed,
    denied: bool,
    position: usize,
    plays: Vec<usize>,
    chime: Vec<i16>,
}

/// A quiet room: real audio, but nothing like the chime.
const ROOM: i16 = 3;

impl FakeTap {
    fn new(transcript: Tee, denied: bool) -> Self {
        let (check, check_feed) = tee();
        Self {
            tee: transcript.fan_out(check),
            check_feed,
            denied,
            position: 0,
            plays: Vec::new(),
            chime: chime::samples(RATE)
                .into_iter()
                .map(|s| (s * f32::from(i16::MAX)).round() as i16)
                .collect(),
        }
    }

    fn sample(&self, t: usize) -> i16 {
        if self.denied {
            return 0;
        }
        let chime = self
            .plays
            .iter()
            .filter_map(|played| t.checked_sub(played + samples(30)))
            .filter_map(|offset| self.chime.get(offset))
            .fold(0i16, |sum, &s| sum.saturating_add(s));
        if chime == 0 { ROOM } else { chime }
    }

    fn recv(&mut self, clock: &FakeClock) -> Result<Vec<i16>, RecvTimeoutError> {
        let chunk: Vec<i16> = (self.position..self.position + samples(10))
            .map(|t| self.sample(t))
            .collect();
        self.position += chunk.len();
        self.tee.offer(&chunk);
        clock.0.set(clock.0.get() + Duration::from_millis(10));
        match self.check_feed.try_recv() {
            Ok(frames) => Ok(frames),
            Err(TryRecvError::Empty) => Err(RecvTimeoutError::Timeout),
            Err(TryRecvError::Disconnected) => Err(RecvTimeoutError::Disconnected),
        }
    }
}

/// Run [`listen`] against a fake tap; returns the verdict, the tap, and
/// everything the transcript's copy of the system stream received.
fn run(denied: bool) -> (ChannelResult, FakeTap, Vec<i16>, Hush) {
    let hush = Hush::default();
    let (transcript, transcript_feed) = tee();
    let tap = RefCell::new(FakeTap::new(transcript.with_hush(hush.clone()), denied));
    let clock = FakeClock(Cell::new(Duration::ZERO));
    let result = listen(
        &clock,
        &hush,
        || {
            let mut tap = tap.borrow_mut();
            let at = tap.position;
            tap.plays.push(at);
        },
        |_left| tap.borrow_mut().recv(&clock),
    );
    let tap = tap.into_inner();
    let mut heard = Vec::new();
    while let Ok(chunk) = transcript_feed.try_recv() {
        heard.extend(chunk);
    }
    (result, tap, heard, hush)
}

#[test]
fn a_granted_tap_is_granted_with_one_chime() {
    let (result, tap, _, _) = run(false);
    assert_eq!(result.state, ChannelState::Granted, "{}", result.detail);
    assert_eq!(tap.plays.len(), 1);
    assert!(
        tap.plays[0] >= samples(u64::from(chime::SETTLE_MILLIS)),
        "the chime waits for the tap to settle, as before the recording did"
    );
}

#[test]
fn a_tap_of_exact_zeros_is_denied_after_every_allowed_play() {
    let (result, tap, _, _) = run(true);
    assert_eq!(result.state, ChannelState::Denied, "{}", result.detail);
    assert_eq!(tap.plays.len(), chime::MAX_PLAYS as usize);
}

#[test]
fn the_transcript_copy_never_hears_the_chime_and_keeps_its_length() {
    let (_, tap, heard, hush) = run(false);
    let played = tap.plays[0];
    assert!(hush.is_held(), "every play holds the hush for its window");
    assert_eq!(
        heard.len(),
        tap.position,
        "hushed frames are silence, never missing, so timestamps stay put"
    );
    assert!(
        heard[..played].iter().all(|&s| s == ROOM),
        "before the chime the transcript gets the real audio"
    );
    assert!(
        heard[played..].iter().all(|&s| s == 0),
        "from the play on, the transcript gets silence where the chime was"
    );
}

#[test]
fn the_hush_window_covers_the_chime_and_its_listen_tail() {
    let window = hush_window().as_millis() as u32;
    assert!(window >= chime::duration_millis() + chime::LISTEN_TAIL_MILLIS);
    assert!(
        window <= 1_500,
        "about a second per play, not the whole check"
    );
}
