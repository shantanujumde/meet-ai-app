//! Play the chime once, listen live, retry at most once.
//!
//! # Why this replaced looping the chime (TUR-14)
//!
//! The first version of the recording-start check covered the tap's settle
//! window by playing [`looped_samples`](super::looped_samples) across all of
//! [`ONSET_TIMEOUT_MILLIS`] — about eleven chimes back to back — and only read
//! the captured audio afterwards. That made the check reliable and the app
//! unbearable: the user heard a burst of chimes every time they pressed
//! record, when the chime is meant to be a single "recording started" cue.
//!
//! The loop existed only because a play timed to start immediately can land
//! inside the tap's unsettled first second (the 1.07 s finding in
//! [`ONSET_TIMEOUT_MILLIS`]). Two changes remove the need for it:
//!
//! - **Wait for the tap to settle before playing.** The clock is the captured
//!   audio itself: once [`SETTLE_MILLIS`] of it has arrived, the tap has been
//!   running past the measured settle point, so one chime is enough.
//! - **Listen while it plays.** Every chunk is checked as it arrives, so the
//!   moment the chime is heard the check is over — nothing more is played and
//!   nothing more is pulled.
//!
//! # Why there is still one retry
//!
//! The settle time was measured once, on one machine. If a tap takes longer
//! than that, the single chime falls inside the unsettled stretch and is
//! lost, and a lost chime on a granted system is the expensive mistake: it
//! sends a user who has permission to the denial screen (SPEC A7, and the
//! asymmetry spelled out on [`DETECT_FLOOR`](super::DETECT_FLOOR)). So an
//! unheard first play earns exactly one more, timed to land no earlier than
//! [`ONSET_TIMEOUT_MILLIS`] into the capture — about twice the measured worst
//! case. That keeps the "no false denied" guarantee the loop gave, while a
//! granted Mac — the common case — still hears one chime, and even a denied
//! one hears only two.

use super::{ONSET_TIMEOUT_MILLIS, Reading, duration_millis, heard, samples_in};

/// How much captured audio the tap must have delivered before the first play.
///
/// The measured settle time is 1.07 s (see [`ONSET_TIMEOUT_MILLIS`]); this
/// adds a margin on top so a slightly slower tap still has settled by the
/// time the chime reaches it. Counted in captured audio, not wall time, so a
/// tap that starts delivering late simply waits longer before playing.
pub const SETTLE_MILLIS: u32 = 1200;

/// How long after a play starts, beyond the chime's own length, to keep
/// listening before calling that play unheard.
///
/// Our output reaches the tap after the device's output latency. Built-in
/// speakers manage that in tens of milliseconds; Bluetooth (AirPods and the
/// like) can take a few hundred. Giving up too soon would call a slow headset
/// a missing chime, so this is generous.
pub const LISTEN_TAIL_MILLIS: u32 = 600;

/// Never play the chime more than this many times per check: one play, plus
/// the one retry that protects against a slow-settling tap.
pub const MAX_PLAYS: u32 = 2;

/// How long a live check waits, in wall time, for the tap's first frame
/// before giving up on it (TUR-72): a tap that never delivers anything must
/// not hang the check, and a slow start (~3 s seen on a granted Mac) must
/// not count against the listen time.
pub const FIRST_FRAME_TIMEOUT_MILLIS: u32 = 5000;

/// Wall time a live check allows on top of [`worst_case_millis`], counted
/// from the first frame (TUR-72): covers a tap that delivers in bursts or a
/// little slower than real time, so only a stalled tap hits the deadline.
pub const DEADLINE_HEADROOM_MILLIS: u32 = 2000;

/// Why [`play_until_heard`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    /// The chime was heard.
    Heard,
    /// The last allowed play was listened to for its whole window, unheard:
    /// the only ending that can honestly be called a denial.
    Window,
    /// `pull` returned `None` first: listening was cut short (or never got
    /// to a play at all).
    Pulled,
}

/// How one check went.
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    /// The verdict reading: `present` means the chime was heard. On failure,
    /// the most informative reading of the last listen window, so the log can
    /// tell "heard nothing" from "heard something that was not ours".
    pub reading: Reading,
    /// How many times `play` was called: 0, 1 or 2.
    pub plays: u32,
    /// Total captured samples pulled. Zero means the tap delivered nothing at
    /// all, which the caller should report as unmeasurable rather than as a
    /// failed check.
    pub captured: usize,
    /// Why the check ended.
    pub finish: Finish,
    /// Captured samples since the latest play started (0 with no play).
    pub listened: usize,
    /// Peak and RMS of everything captured since the first play (0 with no
    /// play). All zeros means the tap delivered bit-exact silence, the
    /// measured denial; real audio without the chime is something else.
    pub peak: f32,
    pub rms: f32,
}

impl Attempt {
    fn new(
        reading: Reading,
        plays: u32,
        captured: &[f32],
        first_play: usize,
        play_start: usize,
        finish: Finish,
    ) -> Self {
        let after = if plays == 0 {
            &[][..]
        } else {
            &captured[first_play..]
        };
        let peak = after.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        let rms = if after.is_empty() {
            0.0
        } else {
            (after.iter().map(|s| s * s).sum::<f32>() / after.len() as f32).sqrt()
        };
        Self {
            reading,
            plays,
            captured: captured.len(),
            finish,
            listened: if plays == 0 {
                0
            } else {
                captured.len() - play_start
            },
            peak,
            rms,
        }
    }
}

/// Captured-audio length, in milliseconds, after which [`play_until_heard`]
/// is guaranteed to have returned as long as `pull` keeps delivering.
///
/// The second play happens no earlier than [`ONSET_TIMEOUT_MILLIS`], and no
/// earlier than the end of the first play's listen window; one more listen
/// window after that ends the check. A caller putting a wall-clock deadline
/// on `pull` should allow this much audio plus one chunk, because the check
/// only looks at the clock once per chunk.
pub fn worst_case_millis() -> u32 {
    let listen = duration_millis() + LISTEN_TAIL_MILLIS;
    ONSET_TIMEOUT_MILLIS.max(SETTLE_MILLIS + listen) + listen
}

/// Play the chime once the tap has settled, listen live, and retry once if
/// it was not heard.
///
/// `play` starts one chime playing on the output; it should return straight
/// away rather than wait for the chime to finish. `pull` returns the next
/// chunk of freshly-captured audio at `sample_rate`, or `None` when there is
/// nothing more to read (the tap stopped, or the caller gave up).
///
/// The clock is the amount of audio pulled so far — tap time, not wall time
/// — because tap time is what the settle finding was measured in:
///
/// 1. Pull until [`SETTLE_MILLIS`] of audio has arrived, then play once.
/// 2. After every chunk, look for the chime in everything captured since
///    that play started. Heard: return at once, with no further pull and no
///    further play.
/// 3. Not heard once the chime's length plus [`LISTEN_TAIL_MILLIS`] has gone
///    by: if this was the first play, play again as soon as the capture
///    reaches [`ONSET_TIMEOUT_MILLIS`]; if it was the last allowed play
///    ([`MAX_PLAYS`]), return the failed reading.
/// 4. `pull` returning `None` ends the check early with whatever the current
///    listen window shows — or everything captured, if nothing was played.
///
/// The check finishes within [`worst_case_millis`] of captured audio.
pub fn play_until_heard(
    sample_rate: u32,
    mut play: impl FnMut(),
    mut pull: impl FnMut() -> Option<Vec<f32>>,
) -> Attempt {
    let settle = samples_in(SETTLE_MILLIS, sample_rate);
    let retry_from = samples_in(ONSET_TIMEOUT_MILLIS, sample_rate);
    let listen_window = samples_in(duration_millis() + LISTEN_TAIL_MILLIS, sample_rate);

    let mut captured: Vec<f32> = Vec::new();
    let mut plays = 0;
    // Where the latest play started, in captured samples. Zero until the
    // first play, so a check that ends before any play looks at everything.
    let mut play_start = 0;
    let mut first_play = 0;
    let mut listening = false;

    while let Some(chunk) = pull() {
        captured.extend(chunk);

        if listening {
            let reading = heard(&captured[play_start..], sample_rate);
            if reading.present {
                return Attempt::new(
                    reading,
                    plays,
                    &captured,
                    first_play,
                    play_start,
                    Finish::Heard,
                );
            }
            if captured.len() - play_start >= listen_window {
                if plays >= MAX_PLAYS {
                    return Attempt::new(
                        reading,
                        plays,
                        &captured,
                        first_play,
                        play_start,
                        Finish::Window,
                    );
                }
                listening = false;
            }
        }

        let due = if plays == 0 { settle } else { retry_from };
        if !listening && captured.len() >= due {
            play();
            plays += 1;
            play_start = captured.len();
            if plays == 1 {
                first_play = play_start;
            }
            listening = true;
        }
    }

    let reading = heard(&captured[play_start..], sample_rate);
    Attempt::new(
        reading,
        plays,
        &captured,
        first_play,
        play_start,
        Finish::Pulled,
    )
}

/// A live check's clock, in time since the check started. Real checks use
/// [`WallClock`]; tests pass a fake one that the fake feed advances.
pub trait Clock {
    fn elapsed(&self) -> std::time::Duration;
}

/// [`Clock`] on the real monotonic clock.
pub struct WallClock(std::time::Instant);

impl WallClock {
    pub fn start() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Clock for WallClock {
    fn elapsed(&self) -> std::time::Duration {
        self.0.elapsed()
    }
}

/// Why a live check stopped listening, for the log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The chime came back.
    Heard,
    /// Every allowed play was listened to in full; the chime never came back.
    Window,
    /// The wall-clock safety net ran out first: no first frame within
    /// [`FIRST_FRAME_TIMEOUT_MILLIS`], or the tap stalled after it.
    Deadline,
    /// The tap's feed disconnected (the tap stopped).
    TapClosed,
}

impl Ended {
    /// The `ended=` value in the log line.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Heard => "heard",
            Self::Window => "window",
            Self::Deadline => "deadline",
            Self::TapClosed => "tap_closed",
        }
    }
}

/// What one live check saw: the attempt plus the wall-clock facts around it.
#[derive(Debug, Clone, PartialEq)]
pub struct Listened {
    pub attempt: Attempt,
    pub ended: Ended,
    /// Wall time from the start of listening to the first non-empty chunk;
    /// `None` if none ever came.
    pub first_frame: Option<std::time::Duration>,
}

impl Listened {
    /// Whether every allowed play was listened to for its whole window, so
    /// a missing chime really means the tap is not passing our audio.
    pub fn listened_fully(&self) -> bool {
        self.attempt.finish == Finish::Window
    }
}

/// Run [`play_until_heard`] against a live feed of 16 kHz `i16` frames.
///
/// `recv(timeout)` waits at most `timeout` for the next chunk, like
/// [`crate::tee::TeeFeed::recv_timeout`]. The wall-clock safety net has two
/// parts, so a late start can never eat the listen time (TUR-72): up to
/// [`FIRST_FRAME_TIMEOUT_MILLIS`] for the first frame, then
/// [`worst_case_millis`] + [`DEADLINE_HEADROOM_MILLIS`] counted from that
/// first frame.
pub fn listen_live(
    sample_rate: u32,
    clock: &impl Clock,
    play: impl FnMut(),
    mut recv: impl FnMut(std::time::Duration) -> Result<Vec<i16>, std::sync::mpsc::RecvTimeoutError>,
) -> Listened {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    let first_frame_limit = Duration::from_millis(u64::from(FIRST_FRAME_TIMEOUT_MILLIS));
    let after_first =
        Duration::from_millis(u64::from(worst_case_millis() + DEADLINE_HEADROOM_MILLIS));
    let mut first_frame: Option<Duration> = None;
    let mut cut: Option<Ended> = None;

    let pull = || loop {
        let limit = first_frame.map_or(first_frame_limit, |at| at + after_first);
        let left = limit.saturating_sub(clock.elapsed());
        if left.is_zero() {
            cut = Some(Ended::Deadline);
            return None;
        }
        match recv(left) {
            Ok(frames) => {
                if frames.is_empty() {
                    continue;
                }
                if first_frame.is_none() {
                    first_frame = Some(clock.elapsed());
                }
                return Some(
                    frames
                        .into_iter()
                        .map(|s| f32::from(s) / f32::from(i16::MAX))
                        .collect(),
                );
            }
            // Re-check the deadline: a timeout normally means it has passed.
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                cut = Some(Ended::TapClosed);
                return None;
            }
        }
    };
    let attempt = play_until_heard(sample_rate, play, pull);
    let ended = match attempt.finish {
        Finish::Heard => Ended::Heard,
        Finish::Window => Ended::Window,
        Finish::Pulled => cut.unwrap_or(Ended::TapClosed),
    };
    Listened {
        attempt,
        ended,
        first_frame,
    }
}

#[cfg(test)]
mod tests;
