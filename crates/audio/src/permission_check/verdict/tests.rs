//! A fake tee feed on a fake clock drives the real listening loop
//! ([`chime::listen_live`]) and the real verdict, so the TUR-72 timing bug
//! reproduces without a tap, an output device or real time passing.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use super::*;
use crate::chime::{Clock, listen_live, worst_case_millis};

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn samples(millis: u64) -> usize {
    (millis * u64::from(RATE) / 1000) as usize
}

struct FakeClock(Cell<Duration>);

impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        self.0.get()
    }
}

/// A fake process tap behind a tee. Chunk `n` arrives at wall time
/// `first_frame_at + n * wall_per_chunk`; `recv` advances the fake clock to
/// that moment, or by the whole timeout if nothing arrives before it.
struct Feed {
    first_frame_at: Duration,
    chunk: usize,
    wall_per_chunk: Duration,
    /// Every sample a bit-exact zero (the measured denial).
    denied: bool,
    /// Output latency: a play shows up this many samples later.
    latency: usize,
    /// Nothing more ever arrives once this much has been delivered.
    stalls_at: Option<usize>,
    /// The feed disconnects once this much has been delivered.
    closes_at: Option<usize>,
    /// Never delivers anything at all.
    silent_tap: bool,
    position: usize,
    chunks: u32,
    plays: Vec<usize>,
    chime: Vec<i16>,
}

impl Feed {
    /// Real-time delivery in 10 ms chunks, starting `first_frame_at` late.
    fn granted(first_frame_at: Duration) -> Self {
        Self {
            first_frame_at,
            chunk: samples(10),
            wall_per_chunk: ms(10),
            denied: false,
            latency: samples(30),
            stalls_at: None,
            closes_at: None,
            silent_tap: false,
            position: 0,
            chunks: 0,
            plays: Vec::new(),
            chime: chime::samples(RATE)
                .into_iter()
                .map(|s| (s * f32::from(i16::MAX)).round() as i16)
                .collect(),
        }
    }

    fn denied(first_frame_at: Duration) -> Self {
        Self {
            denied: true,
            ..Self::granted(first_frame_at)
        }
    }

    fn play(&mut self) {
        self.plays.push(self.position);
    }

    fn recv(&mut self, clock: &FakeClock, timeout: Duration) -> Result<Vec<i16>, RecvTimeoutError> {
        if self.closes_at.is_some_and(|at| self.position >= at) {
            return Err(RecvTimeoutError::Disconnected);
        }
        let now = clock.elapsed();
        let stalled = self.silent_tap || self.stalls_at.is_some_and(|at| self.position >= at);
        let due = self.first_frame_at + self.wall_per_chunk * self.chunks;
        if stalled || due > now + timeout {
            clock.0.set(now + timeout);
            return Err(RecvTimeoutError::Timeout);
        }
        clock.0.set(now.max(due));
        self.chunks += 1;
        let start = self.position;
        self.position += self.chunk;
        Ok((start..self.position).map(|t| self.sample(t)).collect())
    }

    fn sample(&self, t: usize) -> i16 {
        if self.denied {
            return 0;
        }
        self.plays
            .iter()
            .filter_map(|played| t.checked_sub(played + self.latency))
            .filter_map(|offset| self.chime.get(offset))
            .fold(0i16, |sum, &s| sum.saturating_add(s))
    }
}

fn check(feed: Feed) -> (ChannelResult, Listened, Feed, Duration) {
    let clock = FakeClock(Cell::new(Duration::ZERO));
    let feed = RefCell::new(feed);
    let listened = listen_live(
        RATE,
        &clock,
        || feed.borrow_mut().play(),
        |left| feed.borrow_mut().recv(&clock, left),
    );
    (
        system_verdict(&listened),
        listened,
        feed.into_inner(),
        clock.elapsed(),
    )
}

#[test]
fn a_granted_tap_whose_first_frame_is_late_is_granted_with_one_chime() {
    // The owner's Mac (TUR-72): the old deadline started before the first
    // frame, so a ~2.9 s late start left ~0.23 s to listen after the chime
    // and the check said Denied. Counting from the first frame, it is heard.
    for late in [0, 2_000, 2_900, 4_000] {
        let (result, listened, feed, _) = check(Feed::granted(ms(late)));
        assert_eq!(
            result.state,
            ChannelState::Granted,
            "{late} ms: {listened:?}"
        );
        assert_eq!(listened.ended, Ended::Heard);
        assert_eq!(
            feed.plays.len(),
            1,
            "{late} ms: one chime per play, one play"
        );
        assert_eq!(listened.first_frame, Some(ms(late)));
        assert!(listened.attempt.peak > 0.0);
    }
}

#[test]
fn a_granted_tap_delivering_large_slow_chunks_is_granted() {
    // 500 ms of audio per chunk, each 600 ms apart: bursty and slower than
    // real time.
    let mut feed = Feed::granted(ms(500));
    feed.chunk = samples(500);
    feed.wall_per_chunk = ms(600);
    let (result, listened, feed, _) = check(feed);
    assert_eq!(result.state, ChannelState::Granted, "{listened:?}");
    assert_eq!(feed.plays.len(), 1);
}

#[test]
fn a_denied_tap_is_still_denied_however_late_or_slow_it_starts() {
    // TUR-14's rule holds: exact zeros never pass, and a full listen of
    // both plays is the one honest denial.
    let mut slow = Feed::denied(ms(1_000));
    slow.chunk = samples(500);
    slow.wall_per_chunk = ms(600);
    for feed in [Feed::denied(ms(0)), Feed::denied(ms(2_900)), slow] {
        let (result, listened, feed, _) = check(feed);
        assert_eq!(result.state, ChannelState::Denied, "{listened:?}");
        assert_eq!(listened.ended, Ended::Window);
        assert_eq!(feed.plays.len(), chime::MAX_PLAYS as usize);
        assert_eq!((listened.attempt.peak, listened.attempt.rms), (0.0, 0.0));
    }
}

#[test]
fn a_tap_that_stalls_right_after_the_chime_is_unmeasurable_not_denied() {
    // The log's shape: the chime plays at 19 200 samples and only ~3 600
    // more arrive. That is a cut-short listen, never a denial.
    let mut feed = Feed::denied(ms(0));
    feed.stalls_at = Some(22_826);
    let (result, listened, feed, elapsed) = check(feed);
    assert_eq!(result.state, ChannelState::Unmeasurable, "{listened:?}");
    assert_eq!(listened.ended, Ended::Deadline);
    assert_eq!(feed.plays.len(), 1);
    assert!(result.detail.contains("cut short"), "{}", result.detail);
    // The safety net counts from the first frame.
    let net = u64::from(worst_case_millis() + chime::DEADLINE_HEADROOM_MILLIS);
    assert_eq!(elapsed, ms(net));
}

#[test]
fn a_tap_that_closes_mid_listen_is_unmeasurable_and_says_so() {
    let mut feed = Feed::granted(ms(0));
    feed.closes_at = Some(samples(u64::from(chime::SETTLE_MILLIS) + 100));
    let (result, listened, _, _) = check(feed);
    assert_eq!(result.state, ChannelState::Unmeasurable, "{listened:?}");
    assert_eq!(listened.ended, Ended::TapClosed);
    assert!(result.detail.contains("tap closed"), "{}", result.detail);
}

#[test]
fn a_tap_that_stops_before_the_retry_is_unmeasurable() {
    // The first play was listened to in full, but the retry that protects a
    // slow-settling tap never happened: still not an honest denial.
    let mut feed = Feed::denied(ms(0));
    feed.closes_at = Some(samples(u64::from(chime::ONSET_TIMEOUT_MILLIS) - 50));
    let (result, listened, feed, _) = check(feed);
    assert_eq!(feed.plays.len(), 1);
    assert_eq!(result.state, ChannelState::Unmeasurable, "{listened:?}");
}

#[test]
fn a_tap_that_never_delivers_gives_up_after_the_first_frame_timeout() {
    let mut feed = Feed::granted(ms(0));
    feed.silent_tap = true;
    let (result, listened, feed, elapsed) = check(feed);
    assert_eq!(result.state, ChannelState::Unmeasurable);
    assert_eq!(listened.ended, Ended::Deadline);
    assert_eq!(listened.first_frame, None);
    assert!(feed.plays.is_empty());
    assert_eq!(elapsed, ms(u64::from(chime::FIRST_FRAME_TIMEOUT_MILLIS)));
    assert!(result.detail.contains("no audio"), "{}", result.detail);
}

#[test]
fn a_disconnected_feed_before_any_play_is_unmeasurable() {
    let mut feed = Feed::granted(ms(0));
    feed.closes_at = Some(samples(500));
    let (result, listened, feed, _) = check(feed);
    assert_eq!(result.state, ChannelState::Unmeasurable);
    assert_eq!(listened.ended, Ended::TapClosed);
    assert!(feed.plays.is_empty());
}
