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

/// Below this a captured sample is the bit-exact zero a denied tap delivers
/// (FINDINGS §10.1, §10.7). Anything louder is real audio, which only a
/// granted tap can hand over.
pub const NONZERO_PEAK: f32 = 1e-6;

/// What one listen proved, before it is put into words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The chime came back: granted.
    Heard,
    /// The chime was not recognised, but real (non-zero) audio came back
    /// after the first play. A denied tap only ever delivers exact zeros, so
    /// this is a grant too (TUR-84: a Bluetooth output at a rate the tap
    /// misreported pitch-shifted the chime past the detector).
    Flowing,
    /// Exact zeros after every allowed play was listened to in full: the one
    /// honest denial.
    Silent,
    /// Exact zeros, but listening was cut short: proves nothing (TUR-72).
    CutShort,
}

/// The rule, pure: `peak` is the loudest sample captured after the first
/// play (0 with no play). Real audio never blocks recording.
pub fn outcome(heard: bool, peak: f32, listened_fully: bool) -> Outcome {
    if heard {
        Outcome::Heard
    } else if peak > NONZERO_PEAK {
        Outcome::Flowing
    } else if listened_fully {
        Outcome::Silent
    } else {
        Outcome::CutShort
    }
}

/// Whether captured frames per second of wall time are more than 5 % off
/// [`RATE`] (TUR-87): the tap resamples from the wrong rate. Live chunks
/// arrive in bursts of tens of ms, so a healthy check is well inside that.
pub fn rate_is_off(frames_per_s: f64) -> bool {
    (frames_per_s / f64::from(RATE) - 1.0).abs() > 0.05
}

fn outcome_of(listened: &Listened) -> Outcome {
    let attempt = &listened.attempt;
    outcome(
        attempt.reading.present,
        attempt.peak,
        listened.listened_fully(),
    )
}

/// The verdict for one check, from [`outcome`].
///
/// `Denied` only when every allowed play was listened to for its whole
/// window and nothing but exact zeros came back. Real audio without the
/// chime is `Granted` (TUR-84). A silent listen that was cut short — by the
/// deadline or by the tap closing — is `Unmeasurable` and says so: it proves
/// nothing either way, and calling it a denial sends a user who has
/// permission to the denial screen (the TUR-72 bug).
pub fn system_verdict(listened: &Listened) -> ChannelResult {
    let attempt = &listened.attempt;
    match outcome_of(listened) {
        Outcome::Heard => {
            return ChannelResult {
                state: ChannelState::Granted,
                detail: "the check tone was played and recovered from the system-audio recording"
                    .into(),
            };
        }
        Outcome::Flowing => {
            return ChannelResult {
                state: ChannelState::Granted,
                detail: "system audio is flowing; the check tone was not recognised".into(),
            };
        }
        Outcome::Silent => {
            return ChannelResult {
                state: ChannelState::Denied,
                detail: "the check tone did not come back through the system-audio tap".into(),
            };
        }
        Outcome::CutShort => {}
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
/// a grant whose chime did not survive the path (a misread device rate, a
/// muted or wrong output), so it also gets a warning carrying `rates`, the
/// source's [`crate::AudioSource::rate_report`].
pub fn log(listened: &Listened, rates: Option<&str>) {
    let attempt = &listened.attempt;
    tracing::info!(
        plays = attempt.plays,
        captured = attempt.captured,
        present = attempt.reading.present,
        ended = listened.ended.as_str(),
        first_frame_ms = ?listened.first_frame.map(|at| at.as_millis()),
        captured_frames_per_s = ?listened.frames_per_s.map(|hz| hz.round() as u64),
        peak = attempt.peak,
        rms = attempt.rms,
        "system-audio permission check finished"
    );
    if let Some(hz) = listened.frames_per_s.filter(|hz| rate_is_off(*hz)) {
        tracing::warn!(
            captured_frames_per_s = hz.round() as u64,
            expected = RATE,
            rates = rates.unwrap_or("unknown"),
            "the system tap delivered audio at the wrong pace during the permission check: a \
             rate error, not a permission answer"
        );
    }
    tracing::debug!(reading = ?attempt.reading, "system-audio permission check reading");
    if outcome_of(listened) == Outcome::Flowing {
        tracing::warn!(
            peak = attempt.peak,
            rms = attempt.rms,
            rates = rates.unwrap_or("unknown"),
            "system audio is flowing but the check tone was not recognised; counting it as \
             granted"
        );
    }
}

#[cfg(test)]
mod tests;
