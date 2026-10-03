//! The macOS side of [`crate::platform`]: the functions the rest of the crate
//! reaches through that seam, each a thin call into the Core Audio and
//! AVFoundation code in this folder.

use crate::activity::DeviceActivity;
use crate::permission_check::{self, ChannelResult, ChannelState};
use crate::{AudioSource, Error};

pub(crate) use super::device_watch::{default_input_device, default_output_device};

/// A Core Audio device id, as [`default_output_device`] returns it.
pub(crate) type DeviceId = objc2_core_audio::AudioObjectID;

/// [`device_activity`] gives a real reading here.
#[cfg(test)]
pub(crate) const DEVICE_ACTIVITY: bool = true;

/// The resampler's f32 golden hashes (`resample.rs` tests) were taken here.
#[cfg(test)]
pub(crate) const F32_GOLDEN_HASHES: bool = true;

/// Host time "now", in the same clock domain as the process tap's
/// `AudioTimeStamp.mHostTime` converted via `AudioConvertHostTimeToNanos`
/// (contract revision 3, §0: "mach_absolute_time, converted to ns via
/// mach_timebase_info"). `AudioGetCurrentHostTime` returns exactly the value
/// `mach_absolute_time()` would — same underlying counter — so calling it
/// from the mic callback keeps both channels' anchors comparable without
/// needing to reconcile two different clock epochs. `cpal`'s own per-callback
/// `InputCallbackInfo` timestamp is not documented to share that domain, so
/// this reads the Core Audio host clock directly instead of trusting it.
pub(crate) fn host_now_ns() -> u64 {
    // SAFETY: both calls read current host-clock state; neither takes a
    // pointer or has a precondition beyond "the audio HAL is initialized",
    // which it is by the time any cpal stream callback can run.
    unsafe {
        objc2_core_audio::AudioConvertHostTimeToNanos(objc2_core_audio::AudioGetCurrentHostTime())
    }
}

/// The Core Audio process tap.
pub(crate) fn system_source() -> Option<Box<dyn AudioSource>> {
    Some(Box::new(super::tap::SystemSource::new()))
}

/// The `cpal` microphone.
pub(crate) fn mic_source() -> Box<dyn AudioSource> {
    Box::new(crate::mic::MicSource::new())
}

/// Core Audio's "is anyone running this device" flags.
pub(crate) fn device_activity() -> Result<DeviceActivity, Error> {
    super::activity::read()
}

/// Ask macOS directly whether the microphone is authorized, without opening
/// any stream.
///
/// `AVCaptureDevice.authorizationStatus(for: .audio)` is a public, documented,
/// synchronous TCC query — a different mechanism entirely from the process
/// tap's `OSStatus`/sample-payload path that FINDINGS §10.1 proved lies on
/// denial. It reads the same `kTCCServiceMicrophone` record `cpal`'s
/// CoreAudio path is ultimately gated by, for this same process, so a
/// `Denied`/`Restricted` answer here is authoritative.
fn mic_authorization_status() -> objc2_av_foundation::AVAuthorizationStatus {
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: `AVMediaTypeAudio` is an Apple-provided static that is always
    // present once the AVFoundation image is loaded; `authorizationStatusForMediaType`
    // reads TCC state and has no other preconditions.
    unsafe {
        let media_type =
            // quality: allow-unwrap moved as is from permission_check.rs; the static is always set
            AVMediaTypeAudio.expect("AVFoundation always provides the AVMediaTypeAudio constant");
        AVCaptureDevice::authorizationStatusForMediaType(media_type)
    }
}

/// A stored `Denied`/`Restricted` microphone decision, if there is one.
///
/// `None` covers `Authorized` and `NotDetermined` alike: neither says anything
/// about the mic until a stream is actually opened.
pub(crate) fn stored_mic_denial() -> Option<ChannelResult> {
    use objc2_av_foundation::AVAuthorizationStatus;
    let status = mic_authorization_status();
    if status == AVAuthorizationStatus::Denied {
        return Some(ChannelResult {
            state: ChannelState::Denied,
            detail: "macOS reports the microphone permission as explicitly denied \
                     (AVAuthorizationStatusDenied)"
                .into(),
        });
    }
    if status == AVAuthorizationStatus::Restricted {
        return Some(ChannelResult {
            state: ChannelState::Denied,
            detail: "macOS reports the microphone as restricted (parental controls or an \
                     MDM profile), which this client cannot change"
                .into(),
        });
    }
    None
}

/// Open the `cpal` microphone briefly.
pub(crate) fn check_mic() -> ChannelResult {
    permission_check::check_mic_with(mic_source())
}

/// The chime closed loop against the process tap.
pub(crate) fn check_system() -> ChannelResult {
    permission_check::check_system_with(Box::new(super::tap::SystemSource::new()))
}
