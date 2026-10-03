//! Which sample rate the system tap's IO proc really delivers, and noticing
//! when it changes mid-recording (TUR-80, TUR-84).
//!
//! `kAudioTapPropertyFormat` reports the tap's own format (48 kHz on every
//! Mac seen so far), but the IO proc runs on the private aggregate device's
//! clock, and the aggregate's main sub-device is the default output. A
//! Bluetooth headset in a call (HFP) runs that output at 16 kHz, so the IO
//! proc hands over 16 kHz frames; resampling them as 48 kHz kept 1/3 of the
//! audio and squeezed it 3x. A headset also switches from A2DP (48 kHz) to
//! HFP (16 kHz) when a call starts, without any default-device change, so the
//! rate has to be watched for the whole recording, not read once.
//!
//! TUR-84: the aggregate's input stream can still claim 48 kHz while the
//! aggregate and the output run at 16 kHz or 44.1 kHz (realme Buds), and the
//! IO proc delivers at the aggregate's rate. So the device nominal rates now
//! win over the stream format, and the delivered rate is also **measured**
//! (frames vs `mHostTime`, [`RateMeter`]); a measurement more than
//! [`MEASURED_TOLERANCE`] away from the reported rate wins.
//!
//! [`effective_rate`] is the decision, kept pure so it is unit-tested
//! without a device. The rest is the Core Audio reads feeding it, the shared
//! [`RateState`] the IO proc, the worker and the rate listeners
//! ([`RateWatch`]) all see, and those listeners.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
use objc2_core_audio_types::AudioStreamBasicDescription;

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

fn plausible(rate: Option<f64>) -> Option<u32> {
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

/// Every rate Core Audio reports for one tap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ReportedRates {
    /// `kAudioTapPropertyFormat`'s rate.
    pub tap_format: u32,
    /// The aggregate device's `kAudioDevicePropertyNominalSampleRate`.
    pub aggregate_nominal: Option<f64>,
    /// The default output device's nominal rate (the aggregate's main
    /// sub-device, so its clock).
    pub output_nominal: Option<f64>,
    /// The virtual format of the aggregate's last input stream.
    pub stream_virtual: Option<f64>,
}

impl ReportedRates {
    /// The reported rate to trust: the aggregate's nominal rate (the clock
    /// the IO proc runs on), then the output device's (the aggregate's
    /// clock source), then the input stream's format, then the tap's own
    /// format. TUR-80 put the stream first; on the owner's Bluetooth buds
    /// it said 48 kHz while frames came at 16 and 44.1 kHz (TUR-84).
    pub(crate) fn chosen(&self) -> u32 {
        plausible(self.aggregate_nominal)
            .or_else(|| plausible(self.output_nominal))
            .or_else(|| plausible(self.stream_virtual))
            .unwrap_or(self.tap_format)
    }
}

/// The rate to resample the tap's frames from: [`ReportedRates::chosen`],
/// unless `measured` (frames per second of host time) differs from it by
/// more than [`MEASURED_TOLERANCE`], in which case the measurement, snapped
/// to the nearest [`STANDARD_RATES_HZ`], wins.
pub(crate) fn effective_rate(reported: ReportedRates, measured: Option<f64>) -> u32 {
    let chosen = reported.chosen();
    let Some((raw, snapped)) = measured.and_then(|m| Some((m, snap(m)?))) else {
        return chosen;
    };
    let off = (raw - f64::from(chosen)).abs() / f64::from(chosen.max(1));
    if off > MEASURED_TOLERANCE {
        snapped
    } else {
        chosen
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

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: ca::kAudioObjectPropertyElementMain,
    }
}

/// # Safety
/// `T` must match the C layout of `selector`'s value on `object_id`.
unsafe fn read<T: Copy>(object_id: AudioObjectID, selector: u32, scope: u32) -> Option<T> {
    let mut addr = address(selector, scope);
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(value.as_mut_ptr().cast())?,
        )
    };
    // SAFETY: `noErr` with the full size written means Core Audio filled a `T`.
    (status == 0 && size as usize == std::mem::size_of::<T>())
        .then(|| unsafe { value.assume_init() })
}

fn nominal_rate(device_id: AudioObjectID) -> Option<f64> {
    // SAFETY: `kAudioDevicePropertyNominalSampleRate` is a `Float64`.
    unsafe {
        read::<f64>(
            device_id,
            ca::kAudioDevicePropertyNominalSampleRate,
            ca::kAudioObjectPropertyScopeGlobal,
        )
    }
}

/// The virtual format rate of the aggregate's last input stream. The tap is
/// added after the sub-devices, so its stream comes last; every stream of an
/// aggregate runs at the aggregate's one rate anyway.
fn input_stream_rate(device_id: AudioObjectID) -> Option<f64> {
    let mut addr = address(
        ca::kAudioDevicePropertyStreams,
        ca::kAudioObjectPropertyScopeInput,
    );
    let mut size = 0u32;
    // SAFETY: valid address and out-pointer for the duration of the call.
    let status = unsafe {
        ca::AudioObjectGetPropertyDataSize(
            device_id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    if status != 0 || count == 0 {
        return None;
    }
    let mut streams = vec![ca::kAudioObjectUnknown; count];
    let mut size = (count * std::mem::size_of::<AudioObjectID>()) as u32;
    // SAFETY: `streams` holds `size` bytes of `AudioStreamID`s.
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&mut addr),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(streams.as_mut_ptr().cast())?,
        )
    };
    let written = size as usize / std::mem::size_of::<AudioObjectID>();
    if status != 0 || written == 0 {
        return None;
    }
    // SAFETY: `kAudioStreamPropertyVirtualFormat` is an `AudioStreamBasicDescription`.
    let format: AudioStreamBasicDescription = unsafe {
        read(
            streams[written.min(count) - 1],
            ca::kAudioStreamPropertyVirtualFormat,
            ca::kAudioObjectPropertyScopeGlobal,
        )?
    };
    Some(format.mSampleRate)
}

/// The devices whose rates decide [`effective_rate`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct RateSources {
    pub tap_format_rate: u32,
    pub aggregate_id: AudioObjectID,
    pub output_device_id: AudioObjectID,
}

impl RateSources {
    fn read(&self) -> ReportedRates {
        ReportedRates {
            tap_format: self.tap_format_rate,
            aggregate_nominal: nominal_rate(self.aggregate_id),
            output_nominal: nominal_rate(self.output_device_id),
            stream_virtual: input_stream_rate(self.aggregate_id),
        }
    }
}

/// What every rate log line says.
fn describe(reported: &ReportedRates, measured: Option<f64>) -> String {
    let effective = effective_rate(*reported, measured);
    format!(
        "tap format {} Hz, aggregate nominal {:?} Hz, input stream {:?} Hz, output device {:?} \
         Hz, measured_rate {:?} Hz, effective_rate {effective} Hz; resampling from {effective} Hz",
        reported.tap_format,
        reported.aggregate_nominal,
        reported.stream_virtual,
        reported.output_nominal,
        measured.map(|m| m.round() as u32),
    )
}

/// The rates one tap is running at, shared by the rate listeners (which
/// re-read the reported rates), the IO proc (which publishes measurements)
/// and the worker (which resamples at [`RateState::effective`]).
pub(crate) struct RateState {
    sources: RateSources,
    /// Written by the listeners, read by the worker; never by the IO proc.
    reported: Mutex<ReportedRates>,
    /// The latest measurement as `f32` bits, 0 for none yet.
    measured_bits: AtomicU32,
    /// Whether that measurement is [`Measurement::first`].
    measured_first: AtomicBool,
    /// Bumped on every rate-change notification, so the IO proc restarts its
    /// [`RateMeter`].
    epoch: AtomicU32,
    /// Set whenever the effective rate may have changed.
    dirty: AtomicBool,
}

impl RateState {
    /// Read every rate once and log them.
    pub(crate) fn new(sources: RateSources) -> Arc<Self> {
        let reported = sources.read();
        tracing::info!("system tap rates (start): {}", describe(&reported, None));
        Self::with(sources, reported)
    }

    /// A state that starts from `reported` without reading any device, for
    /// tests that drive the IO-proc and worker sides by hand.
    #[cfg(test)]
    pub(crate) fn fake(reported: ReportedRates) -> Arc<Self> {
        let sources = RateSources {
            tap_format_rate: reported.tap_format,
            aggregate_id: ca::kAudioObjectUnknown,
            output_device_id: ca::kAudioObjectUnknown,
        };
        Self::with(sources, reported)
    }

    fn with(sources: RateSources, reported: ReportedRates) -> Arc<Self> {
        Arc::new(Self {
            sources,
            reported: Mutex::new(reported),
            measured_bits: AtomicU32::new(0),
            measured_first: AtomicBool::new(false),
            epoch: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
        })
    }

    fn reported(&self) -> ReportedRates {
        match self.reported.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// The latest measured delivery rate, if a window has completed since
    /// the last rate change.
    pub(crate) fn measured(&self) -> Option<f64> {
        let bits = self.measured_bits.load(Ordering::Acquire);
        (bits != 0).then(|| f64::from(f32::from_bits(bits)))
    }

    /// Whether [`Self::measured`] is the first since the last (re)start.
    pub(crate) fn measured_first(&self) -> bool {
        self.measured_first.load(Ordering::Acquire)
    }

    /// The rate to resample from right now.
    pub(crate) fn effective(&self) -> u32 {
        effective_rate(self.reported(), self.measured())
    }

    /// The current rate log line's text, for a caller's own log.
    pub(crate) fn describe(&self) -> String {
        describe(&self.reported(), self.measured())
    }

    /// Re-read the reported rates and forget the measurement, which the IO
    /// proc then takes again (a rate-change notification).
    fn reread(&self, when: &str) {
        let previous = self.effective();
        let reported = self.sources.read();
        match self.reported.lock() {
            Ok(mut guard) => *guard = reported,
            Err(poisoned) => *poisoned.into_inner() = reported,
        }
        let rate = reported.chosen();
        if previous != rate {
            tracing::warn!(
                "system tap input rate changed {previous} Hz -> {rate} Hz mid-recording"
            );
        }
        self.measured_bits.store(0, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.dirty.store(true, Ordering::Release);
        tracing::info!("system tap rates ({when}): {}", describe(&reported, None));
    }

    /// The meter epoch the IO proc compares against.
    pub(crate) fn epoch(&self) -> u32 {
        self.epoch.load(Ordering::Acquire)
    }

    /// Publish a measurement from the IO proc: two atomic stores, nothing
    /// else, so it is safe on the real-time thread.
    pub(crate) fn publish_measured(&self, measured: Measurement) {
        self.measured_first.store(measured.first, Ordering::Release);
        // Never 0 bits: a plausible rate is >= 1 kHz.
        self.measured_bits
            .store((measured.hz as f32).to_bits(), Ordering::Release);
        self.dirty.store(true, Ordering::Release);
    }

    /// For the worker: whether anything changed since it last asked.
    pub(crate) fn take_change(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
}

/// The IO proc's side of the measurement: a [`RateMeter`] restarted after
/// every rate-change notification, publishing into [`RateState`]. Atomics
/// and integers only, so it is safe on the real-time thread.
pub(crate) struct CallbackMeter {
    rates: Arc<RateState>,
    meter: RateMeter,
    epoch: u32,
    channels: usize,
}

impl CallbackMeter {
    pub(crate) fn new(rates: Arc<RateState>, channels: usize) -> Self {
        let epoch = rates.epoch();
        Self {
            rates,
            meter: RateMeter::default(),
            epoch,
            channels: channels.max(1),
        }
    }

    /// One callback: its input host time and how many interleaved samples
    /// it delivered.
    pub(crate) fn observe(&mut self, host_ns: u64, samples: usize) {
        let epoch = self.rates.epoch();
        if epoch != self.epoch {
            self.epoch = epoch;
            self.meter.reset();
        }
        let frames = (samples / self.channels) as u64;
        if let Some(measured) = self.meter.observe(host_ns, frames) {
            self.rates.publish_measured(measured);
        }
    }
}

/// Listener for `kAudioDevicePropertyNominalSampleRate` on the aggregate and
/// on the output device. Runs on a Core Audio notification thread, never
/// the IO thread, so reading properties here is fine.
unsafe extern "C-unwind" fn on_rate_change(
    _object_id: AudioObjectID,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    // SAFETY: `client_data` is the leaked `Arc<RateState>` from
    // `RateWatch::install`, valid for the rest of the process.
    let Some(state) = (unsafe { client_data.cast::<RateState>().as_ref() }) else {
        return 0;
    };
    state.reread("rate changed");
    0
}

/// The registered rate listeners for one tap. Dropping it unregisters them.
pub(crate) struct RateWatch {
    state: *const RateState,
    objects: Vec<AudioObjectID>,
}

// SAFETY: `state` points at a leaked, never-freed `RateState`, which is
// `Sync` (ids, a `Mutex` and atomics); nothing here is thread-bound.
unsafe impl Send for RateWatch {}

impl RateWatch {
    /// Register the listeners, then re-read the rates once so a change
    /// between the first read and the registration is not missed.
    pub(crate) fn install(state: &Arc<RateState>) -> Self {
        // Leaked on purpose (one reference per segment): Core Audio gives no
        // guarantee that a notification already in flight has returned when
        // `AudioObjectRemovePropertyListener` does, so releasing this on drop
        // could leave that callback reading freed memory.
        let leaked = Arc::into_raw(Arc::clone(state));
        let sources = state.sources;
        let mut objects = Vec::new();
        for object in [sources.aggregate_id, sources.output_device_id] {
            let mut addr = address(
                ca::kAudioDevicePropertyNominalSampleRate,
                ca::kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: `on_rate_change` matches `AudioObjectPropertyListenerProc`,
            // and `leaked` stays valid forever (see above).
            let status = unsafe {
                ca::AudioObjectAddPropertyListener(
                    object,
                    NonNull::from(&mut addr),
                    Some(on_rate_change),
                    leaked.cast_mut().cast(),
                )
            };
            if status == 0 {
                objects.push(object);
            } else {
                tracing::warn!("could not watch the sample rate of device {object}: {status}");
            }
        }
        state.reread("after installing rate listeners");
        Self {
            state: leaked,
            objects,
        }
    }
}

impl Drop for RateWatch {
    fn drop(&mut self) {
        for &object in &self.objects {
            let mut addr = address(
                ca::kAudioDevicePropertyNominalSampleRate,
                ca::kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: the same (object, address, proc, client data) as registered.
            unsafe {
                ca::AudioObjectRemovePropertyListener(
                    object,
                    NonNull::from(&mut addr),
                    Some(on_rate_change),
                    self.state.cast_mut().cast(),
                )
            };
        }
    }
}

#[cfg(test)]
mod tests;
