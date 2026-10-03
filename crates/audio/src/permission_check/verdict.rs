//! Turn one live system-audio check into a [`ChannelResult`] (TUR-72).
//!
//! Platform-free on purpose, so the rule "never call a cut-short listen a
//! denial" is tested headless with a fake feed and a fake clock, without the
//! tap or an output device.

use super::{ChannelResult, ChannelState};
use crate::chime::{self, Ended, Listened};

/// The rate the tee delivers the tap's frames at.
pub const RATE: u32 = 16_000;

fn millis(samples: usize) -> u64 {
    samples as u64 * 1000 / u64::from(RATE)
}

/// The verdict for one check.
///
/// `Denied` only when every allowed play was listened to for its whole
/// window and the chime never came back. A listen that was cut short — by
/// the deadline or by the tap closing — is `Unmeasurable` and says so: it
/// proves nothing either way, and calling it a denial sends a user who has
/// permission to the denial screen (the TUR-72 bug).
pub fn system_verdict(listened: &Listened) -> ChannelResult {
    let attempt = &listened.attempt;
    if attempt.reading.present {
        return ChannelResult {
            state: ChannelState::Granted,
            detail: "the check tone was played and recovered from the system-audio recording"
                .into(),
        };
    }
    if listened.listened_fully() {
        return ChannelResult {
            state: ChannelState::Denied,
            detail: "the check tone did not come back through the system-audio tap".into(),
        };
    }
    // No play means the chime never sounded: the tap delivered nothing at
    // all or stopped before the settle time ran out.
    if attempt.plays == 0 {
        let detail = if listened.first_frame.is_none() && listened.ended == Ended::Deadline {
            format!(
                "the system-audio tap delivered no audio within {} ms, so the check tone was \
                 never played",
                chime::FIRST_FRAME_TIMEOUT_MILLIS
            )
        } else {
            "the system-audio tap stopped delivering audio before the check tone could play".into()
        };
        return ChannelResult {
            state: ChannelState::Unmeasurable,
            detail,
        };
    }
    let why = match listened.ended {
        Ended::Deadline => "the tap stopped delivering audio in time",
        _ => "the tap closed",
    };
    ChannelResult {
        state: ChannelState::Unmeasurable,
        detail: format!(
            "listening was cut short ({why}) after {} of {} check tones: {} ms of audio came \
             back after the last one, and a full listen needs {} ms",
            attempt.plays,
            chime::MAX_PLAYS,
            millis(attempt.listened),
            chime::duration_millis() + chime::LISTEN_TAIL_MILLIS,
        ),
    }
}

/// The one info line every check ends with: why listening ended, how long
/// the first frame took, and how loud the audio after the first play was.
/// All zeros after a play is a real denial; real audio without the tone is
/// something else (muted output, wrong device), which the next real-machine
/// run needs to tell apart.
pub fn log(listened: &Listened) {
    let attempt = &listened.attempt;
    tracing::info!(
        plays = attempt.plays,
        captured = attempt.captured,
        present = attempt.reading.present,
        ended = listened.ended.as_str(),
        first_frame_ms = ?listened.first_frame.map(|at| at.as_millis()),
        peak = attempt.peak,
        rms = attempt.rms,
        "system-audio permission check finished"
    );
    tracing::debug!(reading = ?attempt.reading, "system-audio permission check reading");
}

#[cfg(test)]
mod tests;
