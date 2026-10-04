//! Test-only helpers shared across the workspace crates. Never a normal
//! dependency: crates list it under `[dev-dependencies]`.

use std::f32::consts::PI as PI_F32;
use std::f64::consts::PI as PI_F64;

mod fake_cli;
pub use fake_cli::{FakeCli, fake_cli_path};

/// A mono sine wave as `f32` samples in `[-amplitude, amplitude]`.
pub fn sine_f32(frames: usize, sample_rate: u32, freq_hz: f32, amplitude: f32) -> Vec<f32> {
    (0..frames)
        .map(|n| {
            let t = n as f32 / sample_rate as f32;
            amplitude * (2.0 * PI_F32 * freq_hz * t).sin()
        })
        .collect()
}

/// A mono sine wave as `i16` samples; `amplitude` is a fraction of full scale.
pub fn sine_i16(frames: usize, sample_rate: u32, freq_hz: f64, amplitude: f64) -> Vec<i16> {
    (0..frames)
        .map(|n| {
            let t = n as f64 / sample_rate as f64;
            (i16::MAX as f64 * amplitude * (2.0 * PI_F64 * freq_hz * t).sin()) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_f32_starts_at_zero_and_stays_in_range() {
        let s = sine_f32(480, 48_000, 440.0, 0.5);
        assert_eq!(s.len(), 480);
        assert_eq!(s[0], 0.0);
        assert!(s.iter().all(|x| x.abs() <= 0.5));
    }

    #[test]
    fn sine_i16_starts_at_zero_and_stays_in_range() {
        let s = sine_i16(480, 48_000, 440.0, 0.5);
        assert_eq!(s.len(), 480);
        assert_eq!(s[0], 0);
        assert!(
            s.iter()
                .all(|x| i32::from(*x).abs() <= i32::from(i16::MAX) / 2 + 1)
        );
    }
}
