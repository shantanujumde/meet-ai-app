//! macOS audio capture.
//!
//! SPEC §4 ⛔-marks this directory as the only place OS-specific code may live.
//! The [`AudioSource`](crate::AudioSource) trait in `lib.rs` stays
//! platform-agnostic so the Windows port (SPEC §8.2) is additive rather than a
//! rewrite.
//!
//! The implementation lands in Phase 0. Below the link-time proof tests
//! (which assert the Core Audio entry points FINDINGS §9 committed the
//! project to are reachable from Rust at the versions the workspace pins),
//! [`tap`] is the real, if hardware-unverified, tap implementation.

/// The system-audio [`crate::AudioSource`] — see [`tap::SystemSource`] and its
/// module docs for the honest state of hardware verification.
pub mod tap;

/// Default-output/input-device polling, for detecting the AirPods swap SPEC
/// §5's exit gate names — see the module docs for why this polls instead of
/// registering a Core Audio property listener.
pub mod device_watch;

/// Whether the default mic and speakers are in use by anyone, for the
/// audio-activity meeting signal (TUR-31) — property reads only, no capture.
pub mod activity;

/// The functions [`crate::platform`] routes to on macOS.
pub(crate) mod platform;

/// The real tap/IOProc creation call site (once written) uses
/// [`crate::AUDIO_PERMISSION_TIMEOUT`] — moved there because [`crate::mic`]
/// needs the identical bound on `cpal`'s stream creation, and both are the
/// same failure mode: a Core Audio call that can block on a TCC dialog no one
/// is present to answer.
#[cfg(test)]
mod tests {
    use objc2::AnyThread;
    use objc2::rc::Allocated;
    use objc2_core_audio::{
        AudioDeviceCreateIOProcIDWithBlock, AudioHardwareCreateAggregateDevice,
        AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
        AudioHardwareDestroyProcessTap, AudioObjectID, CATapDescription,
    };

    /// Every Core Audio entry point the Rust capture path needs resolves at
    /// link time.
    ///
    /// FINDINGS §9 chose in-process Rust over a Swift capture sidecar on the
    /// claim that `objc2-core-audio` already binds the process-tap API. That
    /// claim came from reading the crate's source; this asserts it against the
    /// linker, which is the thing that actually fails. If a future version bump
    /// feature-gates one of these away, the build breaks here instead of Phase 0
    /// discovering it mid-implementation.
    ///
    /// It deliberately takes addresses rather than calling. Creating a real tap
    /// would ask TCC for audio-capture permission and `just check` must never
    /// raise a prompt; and per FINDINGS §10.1 the `noErr` such a call returns
    /// proves nothing about capture anyway — only the samples do.
    #[test]
    #[allow(
        function_casts_as_integer,
        reason = "taking the address is the point: it forces the linker to \
                  resolve each symbol, which a type-level reference does not"
    )]
    fn core_audio_process_tap_symbols_link() {
        let entry_points: [(&str, usize); 5] = [
            (
                "AudioHardwareCreateProcessTap",
                AudioHardwareCreateProcessTap as usize,
            ),
            (
                "AudioHardwareDestroyProcessTap",
                AudioHardwareDestroyProcessTap as usize,
            ),
            (
                "AudioHardwareCreateAggregateDevice",
                AudioHardwareCreateAggregateDevice as usize,
            ),
            (
                "AudioHardwareDestroyAggregateDevice",
                AudioHardwareDestroyAggregateDevice as usize,
            ),
            (
                "AudioDeviceCreateIOProcIDWithBlock",
                AudioDeviceCreateIOProcIDWithBlock as usize,
            ),
        ];

        for (name, address) in entry_points {
            assert_ne!(address, 0, "{name} did not resolve");
        }
    }

    /// The two calls that bracket a tap's lifetime have the signatures the
    /// Phase 0 port is being written against.
    ///
    /// The spike (`spikes/phase0a-tcc/`) called these from Swift, where the
    /// compiler imported the shapes from the SDK headers. Rust takes them from
    /// a third-party binding crate instead, so the shapes are worth asserting
    /// rather than assuming: a silent change here is the difference between a
    /// tap over the right process and a tap over nothing.
    #[test]
    fn process_tap_lifetime_calls_have_the_expected_abi() {
        let create: unsafe extern "C-unwind" fn(
            Option<&CATapDescription>,
            *mut AudioObjectID,
        ) -> i32 = AudioHardwareCreateProcessTap;
        let destroy: unsafe extern "C-unwind" fn(AudioObjectID) -> i32 =
            AudioHardwareDestroyProcessTap;

        assert_ne!(create as usize, destroy as usize);
    }

    /// `CATapDescription` — the one Objective-C object in the capture path — is
    /// registered with the runtime, not merely declared in the bindings.
    ///
    /// `alloc` touches the class without configuring or installing a tap, so
    /// like the tests above it cannot trigger a permission prompt.
    #[test]
    fn ca_tap_description_class_is_registered() {
        let allocated = CATapDescription::alloc();
        assert!(!Allocated::as_ptr(&allocated).is_null());
    }
}
