use super::*;

const SR: u32 = 16_000;

/// Deterministic white noise in [-0.5, 0.5) (xorshift), so tests need no rng crate.
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut state = seed.max(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32 / (1u64 << 24) as f32) - 0.5
        })
        .collect()
}

/// `reference` heard later by `lag_at(i)` samples (rounded), silence before.
fn delayed(reference: &[f32], lag_at: impl Fn(usize) -> f64) -> Vec<f32> {
    (0..reference.len())
        .map(|i| {
            let lag = lag_at(i).round() as isize;
            let src = i as isize - lag;
            if src >= 0 && (src as usize) < reference.len() {
                reference[src as usize]
            } else {
                0.0
            }
        })
        .collect()
}

#[test]
fn estimator_finds_a_known_positive_and_negative_lag() {
    let reference = noise(8192, 7);
    let mut estimator = GccPhatLagEstimator::new(4096, 800);
    for lag in [0isize, 37, 640, -25, -700] {
        let observed = delayed(&reference, |_| lag as f64);
        let estimate = estimator
            .estimate(&reference[2048..6144], &observed[2048..6144])
            .expect("a clear peak");
        assert_eq!(estimate.lag_samples, lag);
        assert!(estimate.peak_ratio > 10.0, "{estimate:?}");
        assert!(estimate.distinctiveness > 1.15, "{estimate:?}");
    }
}

#[test]
fn estimator_refuses_wrong_window_length() {
    let mut estimator = GccPhatLagEstimator::new(1024, 100);
    let x = noise(1000, 3);
    assert!(estimator.estimate(&x, &x).is_none());
}

#[test]
fn trend_tracker_reports_ppm_and_ms_per_min() {
    let mut tracker = LagTrendTracker::default();
    assert_eq!(tracker.update(0.0, 0.0, SR), DriftTrendSnapshot::default());
    // 1.6 samples/s at 16 kHz = 100 ppm = 6 ms/min.
    let snap = tracker.update(10.0, 16.0, SR);
    assert!((snap.ppm.unwrap() - 100.0).abs() < 1e-9);
    assert!((snap.ms_per_min.unwrap() - 6.0).abs() < 1e-9);
}

#[test]
fn constant_lag_is_offset_not_drift() {
    let reference = noise(SR as usize * 60, 11);
    let observed = delayed(&reference, |_| 320.0); // 20 ms
    let report = analyze_audio_drift(
        &reference,
        &observed,
        SR,
        &AudioDriftConfig::for_sample_rate(SR),
    )
    .expect("measurable");
    assert!((report.offset_ms() - 20.0).abs() < 0.1, "{report:?}");
    assert!(report.max_drift_ms() < 0.1, "{report:?}");
    assert!(report.fit.ppm.unwrap().abs() < 1.0, "{report:?}");
    assert_eq!(report.locks, 1);
}

#[test]
fn linear_drift_is_measured_in_ppm_and_ms_per_min() {
    // 100 ppm: the mic falls 1.6 samples behind per second, 6 ms per minute.
    let seconds = 120usize;
    let reference = noise(SR as usize * seconds, 5);
    let observed = delayed(&reference, |i| 160.0 + i as f64 * 100e-6);
    let report = analyze_audio_drift(
        &reference,
        &observed,
        SR,
        &AudioDriftConfig::for_sample_rate(SR),
    )
    .expect("measurable");
    let ppm = report.fit.ppm.unwrap();
    assert!((ppm - 100.0).abs() < 5.0, "ppm {ppm}");
    let ms_min = report.fit.ms_per_min.unwrap();
    assert!((ms_min - 6.0).abs() < 0.3, "ms/min {ms_min}");
    assert!((report.offset_ms() - 10.0).abs() < 1.0, "{report:?}");
    // Over ~115 s of trusted points, ~11.5 ms of drift.
    assert!((report.max_drift_ms() - 11.5).abs() < 1.0, "{report:?}");
}

#[test]
fn silence_is_not_measurable() {
    let quiet = vec![0.0f32; SR as usize * 30];
    let err = analyze_audio_drift(&quiet, &quiet, SR, &AudioDriftConfig::for_sample_rate(SR))
        .unwrap_err();
    assert!(
        matches!(err, AudioDriftError::NotEnoughSignal { .. }),
        "{err}"
    );
}

#[test]
fn unrelated_tracks_never_lock() {
    // Headphones: the mic hears something the speakers never played.
    let reference = noise(SR as usize * 60, 1);
    let observed = noise(SR as usize * 60, 2);
    let err = analyze_audio_drift(
        &reference,
        &observed,
        SR,
        &AudioDriftConfig::for_sample_rate(SR),
    )
    .unwrap_err();
    assert!(
        matches!(err, AudioDriftError::NotEnoughSignal { .. }),
        "{err}"
    );
}

#[test]
fn too_short_is_refused() {
    let x = noise(100, 9);
    assert!(matches!(
        analyze_audio_drift(&x, &x, SR, &AudioDriftConfig::for_sample_rate(SR)),
        Err(AudioDriftError::TooShort(_))
    ));
}

#[test]
fn a_lost_lock_is_counted_and_reacquired() {
    let seconds = 120usize;
    let reference = noise(SR as usize * seconds, 21);
    let mut observed = delayed(&reference, |_| 80.0);
    // 30 s of headphones in the middle.
    let other = noise(SR as usize * 30, 22);
    let start = SR as usize * 45;
    observed[start..start + other.len()].copy_from_slice(&other);
    let report = analyze_audio_drift(
        &reference,
        &observed,
        SR,
        &AudioDriftConfig::for_sample_rate(SR),
    )
    .expect("measurable");
    assert_eq!(report.losses, 1, "{report:?}");
    assert_eq!(report.locks, 2, "{report:?}");
    assert!(report.max_drift_ms() < 0.1);
}
