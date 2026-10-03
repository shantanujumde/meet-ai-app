//! A check at every checkpoint that the system track keeps pace with the
//! microphone's (TUR-80).
//!
//! Both channels are 16 kHz after resampling, so over any stretch of a
//! recording they should gain about the same number of frames. When the
//! system tap resampled 16 kHz Bluetooth audio as if it were 48 kHz it gained
//! a third as many, and nothing said so. This only warns in the log; it
//! changes nothing about the recording.

use super::Anchor;

/// How far apart the two channels' frame gains may drift over one checkpoint
/// interval before it is worth a warning. A checkpoint reads each channel's
/// position at its last finished chunk, so a few tens of ms of jitter per
/// 5 s interval is normal; a wrong resampler rate is 50 % or more.
pub const MAX_FRAME_GAIN_MISMATCH: f64 = 0.05;

/// Less than this many mic frames since the previous anchor (0.5 s) is too
/// short a stretch to judge.
const MIN_FRAMES_TO_JUDGE: u64 = 8_000;

/// What [`frame_gain_mismatch`] found: each channel's measured frame rate
/// over the interval, from its own host clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGainMismatch {
    pub mic_frames_per_s: f64,
    pub sys_frames_per_s: f64,
    /// `sys gained / mic gained`; 1.0 is healthy, 0.33 was TUR-80.
    pub ratio: f64,
}

fn per_second(frames: u64, from_ns: u64, to_ns: u64) -> f64 {
    let secs = to_ns.saturating_sub(from_ns) as f64 / 1e9;
    if secs > 0.0 {
        frames as f64 / secs
    } else {
        0.0
    }
}

/// Compare what each channel gained between `previous` and `next`. `None`
/// when they agree, the stretch is too short, or there is no system track.
pub fn frame_gain_mismatch(
    previous: &Anchor,
    next: &Anchor,
    has_system: bool,
) -> Option<FrameGainMismatch> {
    let mic = next.mic_frames.saturating_sub(previous.mic_frames);
    let sys = next.sys_frames.saturating_sub(previous.sys_frames);
    if !has_system || mic < MIN_FRAMES_TO_JUDGE {
        return None;
    }
    let ratio = sys as f64 / mic as f64;
    ((ratio - 1.0).abs() > MAX_FRAME_GAIN_MISMATCH).then(|| FrameGainMismatch {
        mic_frames_per_s: per_second(mic, previous.mic_host_ns, next.mic_host_ns),
        sys_frames_per_s: per_second(sys, previous.sys_host_ns, next.sys_host_ns),
        ratio,
    })
}

/// [`frame_gain_mismatch`], logged as a warning when it finds one.
pub fn warn_if_out_of_step(previous: &Anchor, next: &Anchor, has_system: bool) {
    if let Some(m) = frame_gain_mismatch(previous, next, has_system) {
        tracing::warn!(
            "system track is not keeping pace with the microphone: since the last checkpoint \
             mic gained {:.0} frames/s, system {:.0} frames/s (ratio {:.3}); the system \
             resampler is probably using the wrong input rate",
            m.mic_frames_per_s,
            m.sys_frames_per_s,
            m.ratio
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

    #[test]
    fn channels_in_step_pass() {
        let a = anchor(10, 160_000, 160_000);
        let b = anchor(15, 240_000, 239_700);
        assert_eq!(frame_gain_mismatch(&a, &b, true), None);
    }

    #[test]
    fn the_tur_80_one_third_system_track_is_caught_with_both_rates() {
        // The owner's segments.json: sys gained a third of the mic's frames.
        let a = anchor(10, 160_000, 53_333);
        let b = anchor(15, 240_000, 80_000);
        let m = frame_gain_mismatch(&a, &b, true).expect("must warn");
        assert!((m.mic_frames_per_s - 16_000.0).abs() < 1.0);
        assert!((m.sys_frames_per_s - 5_333.4).abs() < 1.0);
        assert!((m.ratio - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn a_system_track_running_ahead_is_caught_too() {
        let a = anchor(0, 0, 0);
        let b = anchor(5, 80_000, 90_000);
        assert!(frame_gain_mismatch(&a, &b, true).is_some());
    }

    #[test]
    fn no_system_track_or_a_short_stretch_is_not_judged() {
        let a = anchor(0, 0, 0);
        assert_eq!(frame_gain_mismatch(&a, &anchor(5, 80_000, 0), false), None);
        assert_eq!(frame_gain_mismatch(&a, &anchor(0, 4_000, 0), true), None);
    }
}
