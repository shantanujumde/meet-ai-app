//! Which sample rate the system tap's IO proc really delivers, and noticing
//! when it changes mid-recording (TUR-80).
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
//! [`effective_input_rate`] is the decision, kept pure so it is unit-tested
//! without a device. The rest is the Core Audio reads feeding it and a
//! property listener ([`RateWatch`]) that keeps an atomic up to date for the
//! worker thread to pick up at its next chunk boundary.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use objc2_core_audio::{self as ca, AudioObjectID, AudioObjectPropertyAddress};
use objc2_core_audio_types::AudioStreamBasicDescription;

/// Rates outside this range are a failed or nonsense read, never a device.
const PLAUSIBLE_RATES_HZ: std::ops::RangeInclusive<f64> = 1_000.0..=768_000.0;

fn plausible(rate: Option<f64>) -> Option<u32> {
    rate.filter(|r| r.is_finite() && PLAUSIBLE_RATES_HZ.contains(r))
        .map(|r| r.round() as u32)
}

/// The rate to resample the tap's frames from. Prefers the aggregate's input
/// stream format (what the IO proc's buffers are actually in), then the
/// aggregate device's nominal rate (the clock the IO proc runs on), and only
/// then the tap's own reported format, which is what this module used alone
/// before TUR-80 and is wrong whenever the output device is not at 48 kHz.
pub(crate) fn effective_input_rate(
    tap_format_rate: u32,
    aggregate_nominal_rate: Option<f64>,
    stream_virtual_rate: Option<f64>,
) -> u32 {
    plausible(stream_virtual_rate)
        .or_else(|| plausible(aggregate_nominal_rate))
        .unwrap_or(tap_format_rate)
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

/// The devices whose rates decide [`effective_input_rate`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct RateSources {
    pub tap_format_rate: u32,
    pub aggregate_id: AudioObjectID,
    pub output_device_id: AudioObjectID,
}

impl RateSources {
    /// Read every rate, log them all, and return the one to resample from.
    pub(crate) fn read_and_log(&self, when: &str) -> u32 {
        let aggregate = nominal_rate(self.aggregate_id);
        let stream = input_stream_rate(self.aggregate_id);
        let output = nominal_rate(self.output_device_id);
        let effective = effective_input_rate(self.tap_format_rate, aggregate, stream);
        tracing::info!(
            "system tap rates ({when}): tap format {} Hz, aggregate nominal {aggregate:?} Hz, \
             input stream {stream:?} Hz, output device {output:?} Hz; resampling from {effective} Hz",
            self.tap_format_rate
        );
        effective
    }
}

struct WatchState {
    sources: RateSources,
    rate: Arc<AtomicU32>,
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
    // SAFETY: `client_data` is the leaked `WatchState` from `RateWatch::install`,
    // valid for the rest of the process.
    let Some(state) = (unsafe { client_data.cast::<WatchState>().as_ref() }) else {
        return 0;
    };
    let rate = state.sources.read_and_log("rate changed");
    let previous = state.rate.swap(rate, Ordering::AcqRel);
    if previous != rate {
        tracing::warn!("system tap input rate changed {previous} Hz -> {rate} Hz mid-recording");
    }
    0
}

/// The registered rate listeners for one tap. Dropping it unregisters them.
pub(crate) struct RateWatch {
    state: *mut WatchState,
    objects: Vec<AudioObjectID>,
    rate: Arc<AtomicU32>,
}

// SAFETY: `state` points at a leaked, never-freed `WatchState` whose fields
// are `Copy` ids and an `Arc<AtomicU32>`; nothing here is thread-bound.
unsafe impl Send for RateWatch {}

impl RateWatch {
    /// Register the listeners, then re-read the rate once so a change between
    /// the caller's first read and the registration is not missed.
    pub(crate) fn install(sources: RateSources, initial_rate: u32) -> Self {
        let rate = Arc::new(AtomicU32::new(initial_rate));
        // Leaked on purpose (a few dozen bytes per segment): Core Audio gives
        // no guarantee that a notification already in flight has returned
        // when `AudioObjectRemovePropertyListener` does, so freeing this on
        // drop could leave that callback reading freed memory.
        let state = Box::into_raw(Box::new(WatchState {
            sources,
            rate: Arc::clone(&rate),
        }));
        let mut objects = Vec::new();
        for object in [sources.aggregate_id, sources.output_device_id] {
            let mut addr = address(
                ca::kAudioDevicePropertyNominalSampleRate,
                ca::kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: `on_rate_change` matches `AudioObjectPropertyListenerProc`,
            // and `state` stays valid forever (see above).
            let status = unsafe {
                ca::AudioObjectAddPropertyListener(
                    object,
                    NonNull::from(&mut addr),
                    Some(on_rate_change),
                    state.cast(),
                )
            };
            if status == 0 {
                objects.push(object);
            } else {
                tracing::warn!("could not watch the sample rate of device {object}: {status}");
            }
        }
        let rereads = sources.read_and_log("after installing rate listeners");
        rate.store(rereads, Ordering::Release);
        Self {
            state,
            objects,
            rate,
        }
    }

    /// The rate the worker thread should resample from right now.
    pub(crate) fn current(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.rate)
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
                    self.state.cast(),
                )
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_format_wins_over_everything() {
        assert_eq!(
            effective_input_rate(48_000, Some(24_000.0), Some(16_000.0)),
            16_000
        );
    }

    #[test]
    fn the_aggregate_rate_wins_over_the_tap_format() {
        // The TUR-80 Bluetooth case: tap says 48 kHz, the HFP output runs 16 kHz.
        assert_eq!(effective_input_rate(48_000, Some(16_000.0), None), 16_000);
    }

    #[test]
    fn the_tap_format_is_the_last_resort() {
        assert_eq!(effective_input_rate(48_000, None, None), 48_000);
    }

    #[test]
    fn nonsense_reads_fall_through_to_the_next_source() {
        assert_eq!(
            effective_input_rate(48_000, Some(0.0), Some(f64::NAN)),
            48_000
        );
        assert_eq!(
            effective_input_rate(48_000, Some(44_100.0), Some(-1.0)),
            44_100
        );
        assert_eq!(
            effective_input_rate(48_000, Some(f64::INFINITY), Some(1e9)),
            48_000
        );
    }

    #[test]
    fn fractional_rates_round_to_the_nearest_hertz() {
        assert_eq!(effective_input_rate(48_000, None, Some(15_999.6)), 16_000);
    }
}
