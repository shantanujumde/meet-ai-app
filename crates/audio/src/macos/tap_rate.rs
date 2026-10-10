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
use std::sync::{Arc, Mutex};

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
use objc2_core_audio_types::AudioStreamBasicDescription;

#[cfg(test)]
pub(crate) use crate::rate_meter::{CallbackMeter, RateMeter, snap};
pub(crate) use crate::rate_meter::{Measured, Rates, decide, plausible};

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
/// unless a measurement disagrees ([`decide`]).
pub(crate) fn effective_rate(reported: ReportedRates, measured: Option<f64>) -> u32 {
    decide(reported.chosen(), measured)
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
pub(crate) unsafe fn read<T: Copy>(
    object_id: AudioObjectID,
    selector: u32,
    scope: u32,
) -> Option<T> {
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
    let streams = input_streams(device_id)?;
    // SAFETY: `kAudioStreamPropertyVirtualFormat` is an `AudioStreamBasicDescription`.
    let format: AudioStreamBasicDescription = unsafe {
        read(
            *streams.last()?,
            ca::kAudioStreamPropertyVirtualFormat,
            ca::kAudioObjectPropertyScopeGlobal,
        )?
    };
    Some(format.mSampleRate)
}

/// A device's input streams (`kAudioDevicePropertyStreams`, input scope), in
/// the order the IO proc's buffer list carries them. `None` on a failed read.
pub(crate) fn input_streams(device_id: AudioObjectID) -> Option<Vec<AudioObjectID>> {
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
    streams.truncate(written.min(count));
    Some(streams)
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
    /// The IO proc's measurement, restarted on every rate-change notification.
    cell: Measured,
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
            cell: Measured::default(),
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
        self.cell.get()
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
        self.cell.restart();
        tracing::info!("system tap rates ({when}): {}", describe(&reported, None));
    }
}

impl Rates for RateState {
    fn cell(&self) -> &Measured {
        &self.cell
    }

    fn reported_rate(&self) -> u32 {
        self.reported().chosen()
    }

    fn describe(&self) -> String {
        RateState::describe(self)
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
