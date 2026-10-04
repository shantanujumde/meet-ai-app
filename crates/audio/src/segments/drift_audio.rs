//! A second opinion on drift that does not trust our own timestamps: the lag
//! between `system.wav` and `mic.wav` estimated from the audio itself
//! (`drift-check --audio <DIR>`, TUR-64).
//!
//! When the speakers play the other side of the call and the microphone hears
//! it (no headphones), both tracks carry the same sound. GCC-PHAT finds the lag
//! between them in one window; doing that every few seconds gives lag over
//! time, and its slope is the clock drift between the two devices, in ppm and
//! ms per minute. The constant part of the lag (output latency plus the air
//! between speaker and mic) is reported as the offset, not as drift.
//!
//! The windows go through a small lock state machine so that silence, speech
//! the mic hears but the speakers never played, and echo-like false peaks do
//! not become measurements: a lag is only trusted after a few windows agree
//! (`Locked`), and a run of bad windows drops the lock (`Lost`) until a new one
//! is acquired.
//!
//! This never feeds the SPEC §5 gate in `segments.json` mode; it is a check on
//! the anchors, run by hand on a real recording (docs/manual-checks).

use std::collections::VecDeque;
use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Samples either side of the best peak that do not count as a rival peak.
const PEAK_NEIGHBORHOOD: isize = 3;

// Adapted from github.com/fastrepl/anarlog/crates/audio-sync/src/estimator.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// GCC-PHAT lag estimator over fixed-size windows.
///
/// A positive lag means `observed` hears the sound later than `reference`.
pub struct GccPhatLagEstimator {
    window_samples: usize,
    max_lag_samples: usize,
    fft_len: usize,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    reference_time: Vec<f32>,
    observed_time: Vec<f32>,
    reference_freq: Vec<Complex32>,
    observed_freq: Vec<Complex32>,
    cross_freq: Vec<Complex32>,
    correlation: Vec<f32>,
    forward_scratch: Vec<Complex32>,
    inverse_scratch: Vec<Complex32>,
}

impl GccPhatLagEstimator {
    /// `max_lag_samples` is clamped below `window_samples`.
    pub fn new(window_samples: usize, max_lag_samples: usize) -> Self {
        let window_samples = window_samples.max(2);
        let fft_len = (window_samples * 2).next_power_of_two();
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(fft_len);
        let inverse = planner.plan_fft_inverse(fft_len);
        Self {
            window_samples,
            max_lag_samples: max_lag_samples.min(window_samples - 1),
            fft_len,
            reference_time: vec![0.0; fft_len],
            observed_time: vec![0.0; fft_len],
            reference_freq: forward.make_output_vec(),
            observed_freq: forward.make_output_vec(),
            cross_freq: inverse.make_input_vec(),
            correlation: inverse.make_output_vec(),
            forward_scratch: forward.make_scratch_vec(),
            inverse_scratch: inverse.make_scratch_vec(),
            forward,
            inverse,
        }
    }

    /// The lag of `observed` against `reference`, or `None` if either slice
    /// is not exactly one window long or the correlation is flat.
    pub fn estimate(&mut self, reference: &[f32], observed: &[f32]) -> Option<LagEstimate> {
        let n = self.window_samples;
        if reference.len() != n || observed.len() != n {
            return None;
        }
        copy_centered(reference, &mut self.reference_time[..n]);
        self.reference_time[n..].fill(0.0);
        copy_centered(observed, &mut self.observed_time[..n]);
        self.observed_time[n..].fill(0.0);

        self.forward
            .process_with_scratch(
                &mut self.reference_time,
                &mut self.reference_freq,
                &mut self.forward_scratch,
            )
            .ok()?;
        self.forward
            .process_with_scratch(
                &mut self.observed_time,
                &mut self.observed_freq,
                &mut self.forward_scratch,
            )
            .ok()?;

        for ((cross, reference_bin), observed_bin) in self
            .cross_freq
            .iter_mut()
            .zip(self.reference_freq.iter())
            .zip(self.observed_freq.iter())
        {
            let value = *observed_bin * reference_bin.conj();
            let norm = value.norm();
            *cross = if norm > f32::EPSILON {
                value / norm
            } else {
                Complex32::new(0.0, 0.0)
            };
        }
        // The DC and Nyquist bins of a real signal are real; realfft refuses
        // an inverse whose rounding left a stray imaginary part there.
        if let Some(first) = self.cross_freq.first_mut() {
            first.im = 0.0;
        }
        if let Some(last) = self.cross_freq.last_mut() {
            last.im = 0.0;
        }

        self.inverse
            .process_with_scratch(
                &mut self.cross_freq,
                &mut self.correlation,
                &mut self.inverse_scratch,
            )
            .ok()?;

        let max_lag = self.max_lag_samples as isize;
        let mut best_lag = 0isize;
        let mut peak = 0.0f32;
        let mut sum_abs = 0.0f32;
        let mut count = 0usize;
        for lag in -max_lag..=max_lag {
            let value = self.correlation[idx_for_lag(lag, self.fft_len)].abs();
            if value > peak {
                peak = value;
                best_lag = lag;
            }
            sum_abs += value;
            count += 1;
        }
        if count == 0 || peak <= f32::EPSILON {
            return None;
        }
        let noise_floor = (sum_abs - peak).max(0.0) / (count.saturating_sub(1).max(1) as f32);

        let mut second_peak = 0.0f32;
        for lag in -max_lag..=max_lag {
            if (lag - best_lag).abs() <= PEAK_NEIGHBORHOOD {
                continue;
            }
            second_peak = second_peak.max(self.correlation[idx_for_lag(lag, self.fft_len)].abs());
        }

        Some(LagEstimate {
            lag_samples: best_lag,
            peak_ratio: peak / noise_floor.max(1e-6),
            distinctiveness: peak / second_peak.max(1e-6),
        })
    }
}

/// One window's answer: the lag, how far its peak stands above the average
/// (`peak_ratio`) and above the next best peak (`distinctiveness`).
#[derive(Debug, Clone, Copy)]
pub struct LagEstimate {
    pub lag_samples: isize,
    pub peak_ratio: f32,
    pub distinctiveness: f32,
}

fn copy_centered(input: &[f32], output: &mut [f32]) {
    let mean = input.iter().copied().sum::<f32>() / input.len().max(1) as f32;
    for (out, &sample) in output.iter_mut().zip(input.iter()) {
        *out = sample - mean;
    }
}

fn idx_for_lag(lag: isize, fft_len: usize) -> usize {
    if lag >= 0 {
        lag as usize
    } else {
        (fft_len as isize + lag) as usize
    }
}

// Adapted from github.com/fastrepl/anarlog/crates/audio-sync/src/drift.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// Smooths lag-over-time into a drift rate with an EMA (0.8 old, 0.2 new).
#[derive(Debug, Default)]
pub struct LagTrendTracker {
    last_time_s: Option<f64>,
    last_lag_samples: Option<f64>,
    smoothed_samples_per_s: Option<f64>,
}

impl LagTrendTracker {
    pub fn update(
        &mut self,
        time_s: f64,
        lag_samples: f64,
        sample_rate: u32,
    ) -> DriftTrendSnapshot {
        let mut snapshot = DriftTrendSnapshot::default();
        if let (Some(last_time), Some(last_lag)) = (self.last_time_s, self.last_lag_samples) {
            let dt = time_s - last_time;
            if dt > 0.0 {
                let instant = (lag_samples - last_lag) / dt;
                let smoothed = match self.smoothed_samples_per_s {
                    Some(previous) => previous * 0.8 + instant * 0.2,
                    None => instant,
                };
                self.smoothed_samples_per_s = Some(smoothed);
                snapshot = DriftTrendSnapshot::from_rate(smoothed, sample_rate);
            }
        }
        self.last_time_s = Some(time_s);
        self.last_lag_samples = Some(lag_samples);
        snapshot
    }
}

/// A drift rate in the three units people quote it in.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct DriftTrendSnapshot {
    pub samples_per_s: Option<f64>,
    pub ms_per_min: Option<f64>,
    pub ppm: Option<f64>,
}

impl DriftTrendSnapshot {
    fn from_rate(samples_per_s: f64, sample_rate: u32) -> Self {
        let sr = f64::from(sample_rate.max(1));
        Self {
            samples_per_s: Some(samples_per_s),
            ms_per_min: Some(samples_per_s * 60_000.0 / sr),
            ppm: Some(samples_per_s * 1_000_000.0 / sr),
        }
    }
}

/// Tuning for [`analyze_audio_drift`]. The defaults suit 16 kHz meeting audio.
#[derive(Debug, Clone, Copy)]
pub struct AudioDriftConfig {
    /// Samples per GCC-PHAT window.
    pub window_samples: usize,
    /// The largest lag searched, either way.
    pub max_lag_samples: usize,
    /// Samples between the starts of two windows.
    pub interval_samples: usize,
    /// Below this RMS (full scale 1.0) on either track the window is skipped.
    pub min_rms: f32,
    /// Peak thresholds to start trusting a lag, and to keep trusting it.
    pub acquire_peak_ratio: f32,
    pub acquire_distinctiveness: f32,
    pub hold_peak_ratio: f32,
    pub hold_distinctiveness: f32,
    /// Agreeing windows needed to lock, and how far they may spread.
    pub lock_count: usize,
    pub acquire_tolerance_samples: usize,
    /// While locked, a lag further than this from the recent median is an
    /// outlier, not a measurement.
    pub outlier_tolerance_samples: usize,
    /// Consecutive bad windows that drop the lock.
    pub lost_after: usize,
    /// Recent accepted lags the locked median is taken over.
    pub stable_window: usize,
}

impl AudioDriftConfig {
    pub fn for_sample_rate(sample_rate: u32) -> Self {
        let sr = sample_rate as usize;
        Self {
            // ~1 s windows every 5 s, the same cadence as the checkpoint
            // anchors; lags up to ±400 ms, so a breach of the 200 ms gate is
            // still in range to be seen.
            window_samples: (sr + sr / 50).next_power_of_two(),
            max_lag_samples: sr * 2 / 5,
            interval_samples: sr * 5,
            min_rms: 0.003,
            acquire_peak_ratio: 10.0,
            acquire_distinctiveness: 1.15,
            hold_peak_ratio: 8.0,
            hold_distinctiveness: 1.05,
            lock_count: 3,
            acquire_tolerance_samples: sr / 600,
            outlier_tolerance_samples: sr / 100,
            lost_after: 3,
            stable_window: 5,
        }
    }
}

// Adapted from github.com/fastrepl/anarlog/crates/audio-sync/src/probe.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// Where the lock state machine is (anarlog's `SyncProbeState`, with
/// holdover folded into `Locked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    Searching,
    Acquiring,
    Locked,
}

/// One trusted lag measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LagPoint {
    /// Seconds from the start of the tracks to the middle of the window.
    pub time_s: f64,
    pub lag_samples: isize,
}

/// What [`analyze_audio_drift`] measured.
#[derive(Debug, Clone)]
pub struct AudioDrift {
    pub sample_rate: u32,
    pub windows: usize,
    pub low_energy: usize,
    pub weak_correlation: usize,
    pub outliers: usize,
    /// Times a lock was acquired, and dropped.
    pub locks: usize,
    pub losses: usize,
    pub points: Vec<LagPoint>,
    /// Least-squares slope of lag over time.
    pub fit: DriftTrendSnapshot,
    /// The EMA trend after the last point.
    pub ema: DriftTrendSnapshot,
}

impl AudioDrift {
    fn to_ms(&self, samples: f64) -> f64 {
        samples * 1000.0 / f64::from(self.sample_rate.max(1))
    }

    /// The lag at the first trusted point: latency plus air, not drift.
    pub fn offset_ms(&self) -> f64 {
        self.points
            .first()
            .map_or(0.0, |p| self.to_ms(p.lag_samples as f64))
    }

    /// The largest change in lag from the first trusted point: the drift.
    pub fn max_drift_ms(&self) -> f64 {
        let Some(first) = self.points.first() else {
            return 0.0;
        };
        self.points
            .iter()
            .map(|p| self.to_ms((p.lag_samples - first.lag_samples).abs() as f64))
            .fold(0.0, f64::max)
    }

    /// Seconds between the first and last trusted point.
    pub fn span_s(&self) -> f64 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => b.time_s - a.time_s,
            _ => 0.0,
        }
    }
}

/// Why no audio drift number could be given.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioDriftError {
    #[error("the tracks are shorter than one {0}-sample window")]
    TooShort(usize),
    #[error(
        "fewer than two trusted lag measurements in {windows} windows \
         ({low_energy} too quiet, {weak} weak correlation); were headphones on?"
    )]
    NotEnoughSignal {
        windows: usize,
        low_energy: usize,
        weak: usize,
    },
}

/// Estimate drift of `observed` (mic) against `reference` (system) from the
/// audio alone. Both tracks must share `sample_rate`; the shorter one bounds
/// the analysis.
pub fn analyze_audio_drift(
    reference: &[f32],
    observed: &[f32],
    sample_rate: u32,
    config: &AudioDriftConfig,
) -> Result<AudioDrift, AudioDriftError> {
    let window = config.window_samples.max(256);
    let len = reference.len().min(observed.len());
    if len < window {
        return Err(AudioDriftError::TooShort(window));
    }
    let mut estimator = GccPhatLagEstimator::new(window, config.max_lag_samples);
    let mut probe = Probe::new(config);
    let mut out = AudioDrift {
        sample_rate,
        windows: 0,
        low_energy: 0,
        weak_correlation: 0,
        outliers: 0,
        locks: 0,
        losses: 0,
        points: Vec::new(),
        fit: DriftTrendSnapshot::default(),
        ema: DriftTrendSnapshot::default(),
    };
    let sr = f64::from(sample_rate.max(1));
    let mut start = 0usize;
    while start + window <= len {
        let reference_window = &reference[start..start + window];
        let observed_window = &observed[start..start + window];
        let time_s = (start + window / 2) as f64 / sr;
        start += config.interval_samples.max(1);
        out.windows += 1;

        let verdict = if rms(reference_window) < config.min_rms
            || rms(observed_window) < config.min_rms
        {
            out.low_energy += 1;
            None
        } else {
            match estimator.estimate(reference_window, observed_window) {
                Some(estimate) if probe.meets_thresholds(estimate) => Some(estimate.lag_samples),
                _ => {
                    out.weak_correlation += 1;
                    None
                }
            }
        };
        probe.step(time_s, verdict, &mut out);
    }

    if out.points.len() < 2 {
        return Err(AudioDriftError::NotEnoughSignal {
            windows: out.windows,
            low_energy: out.low_energy,
            weak: out.weak_correlation,
        });
    }
    let mut trend = LagTrendTracker::default();
    for p in &out.points {
        out.ema = trend.update(p.time_s, p.lag_samples as f64, sample_rate);
    }
    out.fit = DriftTrendSnapshot::from_rate(least_squares_slope(&out.points), sample_rate);
    Ok(out)
}

/// The lock / lost state machine over window verdicts.
struct Probe<'a> {
    config: &'a AudioDriftConfig,
    state: ProbeState,
    acquiring: VecDeque<LagPoint>,
    stable: VecDeque<isize>,
    rejections: usize,
}

impl<'a> Probe<'a> {
    fn new(config: &'a AudioDriftConfig) -> Self {
        Self {
            config,
            state: ProbeState::Searching,
            acquiring: VecDeque::new(),
            stable: VecDeque::new(),
            rejections: 0,
        }
    }

    fn meets_thresholds(&self, estimate: LagEstimate) -> bool {
        let (peak, distinct) = if self.state == ProbeState::Locked {
            (
                self.config.hold_peak_ratio,
                self.config.hold_distinctiveness,
            )
        } else {
            (
                self.config.acquire_peak_ratio,
                self.config.acquire_distinctiveness,
            )
        };
        estimate.peak_ratio >= peak && estimate.distinctiveness >= distinct
    }

    /// Feed one window: `Some(lag)` if it passed the thresholds.
    fn step(&mut self, time_s: f64, lag: Option<isize>, out: &mut AudioDrift) {
        match (self.state, lag) {
            (ProbeState::Locked, Some(lag)) => {
                let stable = median(&self.stable).unwrap_or(lag);
                if (lag - stable).unsigned_abs() > self.config.outlier_tolerance_samples {
                    out.outliers += 1;
                    self.reject(out);
                } else {
                    self.rejections = 0;
                    self.push_stable(lag);
                    out.points.push(LagPoint {
                        time_s,
                        lag_samples: lag,
                    });
                }
            }
            (ProbeState::Locked, None) => self.reject(out),
            (_, Some(lag)) => {
                let center = median(&self.acquiring.iter().map(|p| p.lag_samples).collect());
                if center.is_some_and(|c| {
                    (lag - c).unsigned_abs() > self.config.acquire_tolerance_samples
                }) {
                    self.acquiring.clear();
                }
                self.acquiring.push_back(LagPoint {
                    time_s,
                    lag_samples: lag,
                });
                self.state = ProbeState::Acquiring;
                if self.acquiring.len() >= self.config.lock_count.max(1) {
                    out.locks += 1;
                    self.state = ProbeState::Locked;
                    self.stable.clear();
                    self.rejections = 0;
                    for point in std::mem::take(&mut self.acquiring) {
                        self.push_stable(point.lag_samples);
                        out.points.push(point);
                    }
                }
            }
            // A bad window while acquiring starts the search over: agreeing
            // windows must be consecutive.
            (_, None) => {
                self.acquiring.clear();
                self.state = ProbeState::Searching;
            }
        }
    }

    fn reject(&mut self, out: &mut AudioDrift) {
        self.rejections += 1;
        if self.rejections >= self.config.lost_after.max(1) {
            out.losses += 1;
            self.state = ProbeState::Searching;
            self.stable.clear();
            self.rejections = 0;
        }
    }

    fn push_stable(&mut self, lag: isize) {
        if self.stable.len() >= self.config.stable_window.max(1) {
            self.stable.pop_front();
        }
        self.stable.push_back(lag);
    }
}

fn median(values: &VecDeque<isize>) -> Option<isize> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<isize> = values.iter().copied().collect();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2
    } else {
        sorted[mid]
    })
}

fn rms(data: &[f32]) -> f32 {
    let energy = data.iter().map(|s| s * s).sum::<f32>() / data.len().max(1) as f32;
    energy.sqrt()
}

/// Slope of lag (samples) over time (s); 0 for fewer than two distinct times.
fn least_squares_slope(points: &[LagPoint]) -> f64 {
    let n = points.len() as f64;
    if points.len() < 2 {
        return 0.0;
    }
    let mean_t = points.iter().map(|p| p.time_s).sum::<f64>() / n;
    let mean_l = points.iter().map(|p| p.lag_samples as f64).sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for p in points {
        let dt = p.time_s - mean_t;
        num += dt * (p.lag_samples as f64 - mean_l);
        den += dt * dt;
    }
    if den > 0.0 { num / den } else { 0.0 }
}

#[cfg(test)]
mod tests;
