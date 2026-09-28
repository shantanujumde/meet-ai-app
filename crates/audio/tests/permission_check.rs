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
