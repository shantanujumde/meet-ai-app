//! A check at every checkpoint that each track keeps pace with the host
//! clock (TUR-80, TUR-87).
//!
//! Both channels are 16 kHz after resampling, so over any stretch of a
//! recording each should gain about [`SAMPLE_RATE_HZ`] frames per second of
//! its own host time. When the system tap resampled 16 kHz Bluetooth audio as
//! if it were 48 kHz it gained a third as many, and nothing said so.
//!
//! TUR-80's first version compared the system track with the microphone's,
//! so a wrong microphone rate blamed the system track, and both wrong by the
//! same factor passed. Each anchor already carries each channel's host time,
//! so each channel is now judged on its own, the way `segments::drift` does
//! after the fact. This only warns in the log; it changes nothing about the
//! recording.

use super::{Anchor, SAMPLE_RATE_HZ};

/// How far (as a fraction) a channel's frames per second of host time may be
/// from [`SAMPLE_RATE_HZ`] over one checkpoint interval before it is worth a
/// warning. A checkpoint reads each channel's position at its last finished
/// chunk, so a few tens of ms of jitter per 5 s interval is normal; a wrong
/// resampler rate is 8 % (44.1 vs 48 kHz) or more.
pub const MAX_RATE_ERROR: f64 = 0.05;

/// Less host time than this since the previous anchor is too short to judge.
const MIN_SPAN_NS: u64 = 500_000_000;

/// Which track [`off_rate`] found off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Track {
    Microphone,
    System,
}

impl Track {
    fn label(self) -> &'static str {
        match self {
            Track::Microphone => "microphone",
            Track::System => "system",
        }
    }
}

/// One track gaining frames at the wrong pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffRate {
    pub track: Track,
    /// Frames gained per second of that track's own host time.
    pub frames_per_s: f64,
}

/// Frames per second of host time over a span, or `None` when it is too short.
fn per_second(frames: u64, from_ns: u64, to_ns: u64) -> Option<f64> {
    let span = to_ns.checked_sub(from_ns)?;
    (span >= MIN_SPAN_NS).then(|| frames as f64 * 1e9 / span as f64)
}

fn judge(track: Track, frames: u64, from_ns: u64, to_ns: u64) -> Option<OffRate> {
    let frames_per_s = per_second(frames, from_ns, to_ns)?;
    let error = (frames_per_s / f64::from(SAMPLE_RATE_HZ) - 1.0).abs();
    (error > MAX_RATE_ERROR).then_some(OffRate {
        track,
        frames_per_s,
    })
}

/// Every track whose pace between `previous` and `next` is off by more than
/// [`MAX_RATE_ERROR`]: the microphone always, the system track when there
/// is one. Empty when both keep pace or a stretch is too short to judge.
pub fn off_rate(previous: &Anchor, next: &Anchor, has_system: bool) -> Vec<OffRate> {
    let mic = judge(
        Track::Microphone,
        next.mic_frames.saturating_sub(previous.mic_frames),
        previous.mic_host_ns,
        next.mic_host_ns,
    );
    let sys = has_system
        .then(|| {
            judge(
                Track::System,
                next.sys_frames.saturating_sub(previous.sys_frames),
                previous.sys_host_ns,
                next.sys_host_ns,
            )
        })
        .flatten();
    mic.into_iter().chain(sys).collect()
}

/// [`off_rate`], each finding logged as a warning naming its track.
pub fn warn_if_out_of_step(previous: &Anchor, next: &Anchor, has_system: bool) {
    for off in off_rate(previous, next, has_system) {
        let track = off.track.label();
        tracing::warn!(
            "{track} track is not keeping pace with the host clock: since the last checkpoint \
             it gained {:.0} frames/s, not {SAMPLE_RATE_HZ}; the {track} resampler is \
             probably using the wrong input rate",
            off.frames_per_s,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;

    fn anchor(host_s: u64, mic_frames: u64, sys_frames: u64) -> Anchor {
        Anchor {
            mic_host_ns: host_s * S,
            mic_frames,
            sys_host_ns: host_s * S,
            sys_frames,
        }
    }

    fn tracks(found: &[OffRate]) -> Vec<Track> {
        found.iter().map(|off| off.track).collect()
    }

    #[test]
    fn channels_in_step_pass() {
        let a = anchor(10, 160_000, 160_000);
        let b = anchor(15, 240_000, 239_700);
        assert_eq!(off_rate(&a, &b, true), vec![]);
    }

    #[test]
    fn the_tur_80_one_third_system_track_names_the_system_track() {
        // The owner's segments.json: sys gained a third of the mic's frames.
        let a = anchor(10, 160_000, 53_333);
        let b = anchor(15, 240_000, 80_000);
        let found = off_rate(&a, &b, true);
        assert_eq!(tracks(&found), vec![Track::System]);
        assert!((found[0].frames_per_s - 5_333.4).abs() < 1.0);
    }

    #[test]
    fn a_wrong_microphone_rate_names_the_microphone_not_the_system_track() {
        // TUR-87 M3: the mic resampled at 48 kHz while the headset ran 16 kHz.
        let a = anchor(0, 0, 0);
        let b = anchor(5, 80_000 / 3, 80_000);
        assert_eq!(tracks(&off_rate(&a, &b, true)), vec![Track::Microphone]);
    }

    #[test]
    fn both_wrong_by_the_same_factor_names_both() {
        // The ratio check passed this: sys/mic = 1.0.
        let a = anchor(0, 0, 0);
        let b = anchor(5, 80_000 * 3, 80_000 * 3);
        assert_eq!(
            tracks(&off_rate(&a, &b, true)),
            vec![Track::Microphone, Track::System]
        );
    }

    #[test]
    fn a_system_track_running_ahead_is_caught_too() {
        let a = anchor(0, 0, 0);
        let b = anchor(5, 80_000, 90_000);
        assert_eq!(tracks(&off_rate(&a, &b, true)), vec![Track::System]);
    }

    #[test]
    fn no_system_track_or_a_short_stretch_is_not_judged() {
        let a = anchor(0, 0, 0);
        assert_eq!(off_rate(&a, &anchor(5, 80_000, 0), false), vec![]);
        let short = Anchor {
            mic_host_ns: 400_000_000,
            mic_frames: 1_000,
            sys_host_ns: 400_000_000,
            sys_frames: 1_000,
        };
        assert_eq!(off_rate(&a, &short, true), vec![]);
    }

    #[test]
    fn a_host_clock_going_backwards_is_not_judged() {
        let a = anchor(10, 0, 0);
        let b = anchor(5, 80_000, 80_000);
        assert_eq!(off_rate(&a, &b, true), vec![]);
    }
}
