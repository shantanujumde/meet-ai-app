//! Measuring the rate a capture device really delivers, from the frames each
//! callback carries against the host clock (TUR-84), for every channel.
//!
//! TUR-84 built this for the system tap, where Core Audio's reported rates
//! can all claim 48 kHz while the IO proc delivers 16 or 44.1 kHz. TUR-87
//! moved it here, out of `macos/`, because the microphone has the same
//! problem: a Bluetooth headset's mic changes rate when another app opens a
//! call (HFP) with no default-device change, so `cpal`'s
//! `default_input_config()` rate, read once, goes stale.
//!
//! [`RateMeter`] is the measurement (integers only, so it runs in a
//! real-time callback); [`Measured`] is the lock-free cell it publishes to;
//! [`Rates`] is what the worker-side [`crate::pipeline::Pipeline`] follows;
//! [`CallbackMeter`] is the callback's side; and [`FixedRates`] is the
//! microphone's [`Rates`]: one reported rate, plus the measurement.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Rates outside this range are a failed or nonsense read, never a device.
const PLAUSIBLE_RATES_HZ: std::ops::RangeInclusive<f64> = 1_000.0..=768_000.0;

/// The rates a measurement snaps to.
pub(crate) const STANDARD_RATES_HZ: [u32; 9] = [
    8_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 88_200, 96_000,
];

/// A measured rate further than this (as a fraction) from the reported one
/// overrides it.
pub(crate) const MEASURED_TOLERANCE: f64 = 0.02;

/// How much host time one measurement spans.
pub(crate) const MEASURE_WINDOW_NS: u64 = 500_000_000;

pub(crate) fn plausible(rate: Option<f64>) -> Option<u32> {
    rate.filter(|r| r.is_finite() && PLAUSIBLE_RATES_HZ.contains(r))
        .map(|r| r.round() as u32)
}

/// The nearest standard rate to a measured one, or `None` for nonsense.
pub(crate) fn snap(rate: f64) -> Option<u32> {
    plausible(Some(rate))?;
    STANDARD_RATES_HZ.iter().copied().min_by(|a, b| {
        let da = (f64::from(*a) - rate).abs();
        let db = (f64::from(*b) - rate).abs();
        da.total_cmp(&db)
    })
}

/// The rate to resample from: `reported`, unless `measured` (frames per
/// second of host time) differs from it by more than [`MEASURED_TOLERANCE`],
/// in which case the measurement, snapped to the nearest
/// [`STANDARD_RATES_HZ`], wins.
pub(crate) fn decide(reported: u32, measured: Option<f64>) -> u32 {
    let Some((raw, snapped)) = measured.and_then(|m| Some((m, snap(m)?))) else {
        return reported;
    };
    let off = (raw - f64::from(reported)).abs() / f64::from(reported.max(1));
    if off > MEASURED_TOLERANCE {
        snapped
    } else {
        reported
    }
}

/// One published measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measurement {
    /// Frames per second of host time.
    pub hz: f64,
    /// The first since the meter was (re)started. The rate in use was then
    /// a guess, so the frames since the start or the last notification were
    /// at this rate all along. A later one is a switch that happened without
    /// a notification.
    pub first: bool,
}

/// Measures the rate the IO proc really delivers: frames counted between
/// callbacks against the callbacks' `mHostTime`, over windows of at least
/// [`MEASURE_WINDOW_NS`].
///
/// No allocation and no locks, so it runs inside the IO proc. The first
/// window after a [`reset`](Self::reset) is published at once; after that a
/// new rate is only published once two windows in a row agree on it, so a
/// window straddling a switch (or a dropout) never flips the resampler.
#[derive(Debug, Default, Clone)]
pub(crate) struct RateMeter {
    /// Host time of the callback the current window started at.
    anchor_ns: Option<u64>,
    /// Frames delivered from the anchor callback up to (not including) the
    /// latest one.
    frames: u64,
    published: Option<u32>,
    candidate: Option<u32>,
}

impl RateMeter {
    /// Start over, e.g. after a rate-change notification.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// One IO callback: its input `mHostTime` in ns and how many frames it
    /// carried. Returns a measurement when there is a new one to publish.
    pub(crate) fn observe(&mut self, host_ns: u64, frames: u64) -> Option<Measurement> {
        let Some(anchor) = self.anchor_ns else {
            self.anchor_ns = Some(host_ns);
            self.frames = frames;
            return None;
        };
        let Some(elapsed) = host_ns.checked_sub(anchor) else {
            // The host clock went backwards: start this window over.
            self.anchor_ns = Some(host_ns);
            self.frames = frames;
            return None;
        };
        if elapsed < MEASURE_WINDOW_NS {
            self.frames += frames;
            return None;
        }
        let rate = self.frames as f64 * 1e9 / elapsed as f64;
        self.anchor_ns = Some(host_ns);
        self.frames = frames;
        let snapped = snap(rate)?;
        match self.published {
            None => {}
            Some(published) if published == snapped => {
                self.candidate = None;
                return None;
            }
            Some(_) if self.candidate != Some(snapped) => {
                self.candidate = Some(snapped);
                return None;
            }
            Some(_) => {}
        }
        let first = self.published.is_none();
        self.published = Some(snapped);
        self.candidate = None;
        Some(Measurement { hz: rate, first })
    }
}

/// The latest measurement, shared lock-free between the callback that
/// publishes it and the worker that resamples by it.
#[derive(Debug, Default)]
pub(crate) struct Measured {
    /// The latest measurement as `f32` bits, 0 for none yet.
    bits: AtomicU32,
    /// Whether that measurement is [`Measurement::first`].
    first: AtomicBool,
    /// Bumped on every restart, so the callback restarts its [`RateMeter`].
    epoch: AtomicU32,
    /// Set whenever the effective rate may have changed.
    dirty: AtomicBool,
}

impl Measured {
    /// The latest measured delivery rate, if a window has completed since
    /// the last restart.
    pub(crate) fn get(&self) -> Option<f64> {
        let bits = self.bits.load(Ordering::Acquire);
        (bits != 0).then(|| f64::from(f32::from_bits(bits)))
    }

    /// Whether [`Self::get`] is the first since the last (re)start.
    pub(crate) fn first(&self) -> bool {
        self.first.load(Ordering::Acquire)
    }

    /// The meter epoch the callback compares against.
    pub(crate) fn epoch(&self) -> u32 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Publish from the callback: atomic stores only, real-time safe.
    pub(crate) fn publish(&self, measured: Measurement) {
        self.first.store(measured.first, Ordering::Release);
        // Never 0 bits: a plausible rate is >= 1 kHz.
        self.bits
            .store((measured.hz as f32).to_bits(), Ordering::Release);
        self.dirty.store(true, Ordering::Release);
    }

    /// Forget the measurement and have the callback measure again (a
    /// rate-change notification).
    // Only the macOS tap gets rate-change notifications today.
    #[allow(dead_code)]
    pub(crate) fn restart(&self) {
        self.bits.store(0, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
    }

    /// For the worker: whether anything changed since it last asked.
    pub(crate) fn take_change(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
}

/// What a worker-side pipeline follows: a reported rate, the measurement,
/// and a log line naming both.
pub(crate) trait Rates: Send + Sync {
    /// The measurement cell the callback publishes into.
    fn cell(&self) -> &Measured;

    /// The rate the device reports, to use unless a measurement disagrees.
    fn reported_rate(&self) -> u32;

    /// Every rate, as one log line.
    fn describe(&self) -> String;

    /// The rate to resample from right now.
    fn effective(&self) -> u32 {
        decide(self.reported_rate(), self.cell().get())
    }
}

/// The callback's side of the measurement: a [`RateMeter`] restarted after
/// every [`Measured::restart`], publishing into the [`Rates`]' cell. Atomics
/// and integers only, so it is safe on a real-time thread.
pub(crate) struct CallbackMeter<R: Rates> {
    rates: Arc<R>,
    meter: RateMeter,
    epoch: u32,
    channels: usize,
}

impl<R: Rates> CallbackMeter<R> {
    pub(crate) fn new(rates: Arc<R>, channels: usize) -> Self {
        let epoch = rates.cell().epoch();
        Self {
            rates,
            meter: RateMeter::default(),
            epoch,
            channels: channels.max(1),
        }
    }

    /// One callback: its host time and how many interleaved samples it
    /// delivered.
    pub(crate) fn observe(&mut self, host_ns: u64, samples: usize) {
        let cell = self.rates.cell();
        let epoch = cell.epoch();
        if epoch != self.epoch {
            self.epoch = epoch;
            self.meter.reset();
        }
        let frames = (samples / self.channels) as u64;
        if let Some(measured) = self.meter.observe(host_ns, frames) {
            cell.publish(measured);
        }
    }
}

/// [`Rates`] for a device that reports one rate up front and nothing after,
/// like a `cpal` input stream: the measurement is the only way to notice it
/// running at another rate.
#[derive(Debug)]
pub(crate) struct FixedRates {
    /// What the log calls this channel.
    name: &'static str,
    reported: u32,
    measured: Measured,
}

impl FixedRates {
    pub(crate) fn new(name: &'static str, reported: u32) -> Arc<Self> {
        Arc::new(Self {
            name,
            reported,
            measured: Measured::default(),
        })
    }
}

impl Rates for FixedRates {
    fn cell(&self) -> &Measured {
        &self.measured
    }

    fn reported_rate(&self) -> u32 {
        self.reported
    }

    fn describe(&self) -> String {
        let measured = self.measured.get();
        let effective = decide(self.reported, measured);
        format!(
            "{} device {} Hz, measured_rate {:?} Hz, effective_rate {effective} Hz; resampling \
             from {effective} Hz",
            self.name,
            self.reported,
            measured.map(|m| m.round() as u32),
        )
    }
}

#[cfg(test)]
mod tests;
