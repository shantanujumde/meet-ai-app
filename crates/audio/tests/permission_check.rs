//! Real-hardware verification of [`audio::permission_check`] — the
//! orchestration `system_closed_loop.rs` and `mic_closed_loop.rs` don't cover,
//! because it's new: scratch-file handling, and folding a channel reading into
//! [`ChannelState`].
//!
//! `#[ignore]`d for the same reason as the closed-loop tests: real hardware,
//! real TCC state, nothing CI can provide. Run explicitly, once against a
//! granted app and once after revoking the grant in System Settings:
//! `cargo test -p audio --test permission_check -- --ignored --nocapture`.

#![cfg(target_os = "macos")]

use audio::permission_check::{ChannelState, check_mic, check_system};

#[test]
#[ignore = "needs a real mic and its TCC grant to already be decided"]
fn check_mic_matches_this_machines_actual_grant() {
    let result = check_mic();
    eprintln!("mic: {result:?}");
    assert_ne!(
        result.state,
        ChannelState::Unmeasurable,
        "expected a real answer, not a failure to check at all: {result:?}"
    );
}

/// TUR-127: on a machine where this binary's microphone TCC grant has been
/// explicitly denied (System Settings → Privacy & Security → Microphone,
/// toggle this binary OFF — not just `tccutil reset`, which only clears back
/// to "not determined"), `check_mic` must report `Denied`. Before TUR-127's
/// fix, `cpal`'s `build_input_stream` opened cleanly and captured real,
/// non-zero audio despite the stored denial, so this reported `Granted`.
#[test]
#[ignore = "needs a real mic with this binary's grant explicitly set to Denied in System Settings"]
fn check_mic_reports_denied_after_an_explicit_denial_not_granted() {
    let result = check_mic();
    eprintln!("mic: {result:?}");
    assert_eq!(
        result.state,
        ChannelState::Denied,
        "an explicitly denied grant must never read back as anything else: {result:?}"
    );
}

#[test]
#[ignore = "needs a real output device and its TCC grant to already be decided"]
fn check_system_matches_this_machines_actual_grant() {
    let result = check_system();
    eprintln!("system audio: {result:?}");
    assert_ne!(
        result.state,
        ChannelState::Unmeasurable,
        "expected a real answer, not a failure to check at all: {result:?}"
    );
}
