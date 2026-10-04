//! The permission checks on Linux (TUR-51).
//!
//! Linux desktops have no audio permission system: any process the user runs
//! may open the microphone and the sound server's monitor sources. Both
//! channels are [`ChannelState::NotApplicable`], which counts as allowed.
//! Nothing is opened here, so a capture that fails later is reported as the
//! device error it is, never as a permission problem.

use crate::permission_check::{ChannelResult, ChannelState};

fn not_applicable(what: &str) -> ChannelResult {
    ChannelResult {
        state: ChannelState::NotApplicable,
        detail: format!("Linux has no permission switch for {what}"),
    }
}

/// No stored decision exists to read.
pub(crate) fn stored_mic_denial() -> Option<ChannelResult> {
    None
}

pub(crate) fn check_mic() -> ChannelResult {
    not_applicable("the microphone")
}

pub(crate) fn check_system() -> ChannelResult {
    not_applicable("system audio")
}
