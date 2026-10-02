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
    let mut listening = false;

    while let Some(chunk) = pull() {
        captured.extend(chunk);

        if listening {
            let reading = heard(&captured[play_start..], sample_rate);
            if reading.present {
                return Attempt {
                    reading,
                    plays,
                    captured: captured.len(),
                };
            }
            if captured.len() - play_start >= listen_window {
                if plays >= MAX_PLAYS {
                    return Attempt {
                        reading,
                        plays,
                        captured: captured.len(),
                    };
                }
                listening = false;
            }
        }

        let due = if plays == 0 { settle } else { retry_from };
        if !listening && captured.len() >= due {
            play();
            plays += 1;
            play_start = captured.len();
            listening = true;
        }
    }

    Attempt {
        reading: heard(&captured[play_start..], sample_rate),
        plays,
        captured: captured.len(),
    }
}

#[cfg(test)]
mod tests;
