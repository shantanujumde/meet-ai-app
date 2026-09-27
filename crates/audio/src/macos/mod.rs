//! macOS audio capture.
//!
//! SPEC §4 ⛔-marks this directory as the only place OS-specific code may live.
//! The [`AudioSource`](crate::AudioSource) trait in `lib.rs` stays
//! platform-agnostic so the Windows port (SPEC §8.2) is additive rather than a
//! rewrite.
//!
//! The implementation lands in Phase 0. What is here today is the link-time
//! proof that the Core Audio entry points FINDINGS §9 committed the project to
//! are reachable from Rust at the versions the workspace pins.

use std::time::Duration;

/// How long the real tap/IOProc creation call site (once written) is allowed
/// to sit inside `AudioHardwareCreateProcessTap` / `AudioDeviceCreateIOProcIDWithBlock`
/// before giving up.
///
/// These calls block for as long as the user takes to answer the TCC consent
/// dialog, not for how long the API itself needs. Tess measured
/// `create_ioproc` at 1393 ms and 2059 ms against real dialogs, versus 5.7 ms
/// warm with no dialog shown (TUR-4, `spikes/phase0a-tcc`). A "safe-looking"
/// 1 s guard would abort the one call that is about to succeed — and only on
/// the very first run, since every later run is warm and invisible to that
/// bug in testing.
///
/// So this is sized for a human, not an API: generous enough that a person
/// reading the dialog never trips it, with the failure mode of "the user
/// walked away from the prompt" being an acceptable one to eventually time
/// out on.
pub const TAP_CREATION_TIMEOUT: Duration = Duration::from_secs(30);

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
