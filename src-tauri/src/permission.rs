//! Audio-capture permission, as the onboarding flow sees it.
//!
//! **This module owns the screen, not the measurement.** The measurement is the
//! Phase 0a/0 problem and belongs to `crates/audio` (TUR-4/TUR-5). SPEC §8.1 is
//! blunt about why it is hard: on denial *every* `OSStatus` is `noErr` and the
//! process tap delivers bit-exact zeros, which is indistinguishable from a
//! granted capture of a silent Mac. So a return code is not evidence and an RMS
//! floor is not evidence. The only proof is a positive control — play a ~200 ms
//! known tone from meet-ai's own process and confirm it comes back through the
//! tap.
//!
//! Until that lands, this reports [`State::Unknown`] and says so in words. It
//! does **not** report `Granted` optimistically: an onboarding screen that
//! claims permission it has not measured is worse than one that admits it does
//! not know, because the user only finds out when a real meeting records
//! silence.

use serde::{Deserialize, Serialize};

/// Where the user stands with audio permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Not measured yet. The honest answer until the positive-control tone
    /// check exists.
    Unknown,
    /// The positive control came back. Recording will capture real audio.
    Granted,
    /// The user said No, or the tone did not come back.
    Denied,
}

/// The permission answer plus enough context for the screen to explain itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: State,
    /// Whether a real measurement produced this, or it is the placeholder.
    ///
    /// The onboarding screen shows different copy for "we checked and you are
    /// fine" versus "we cannot check yet", and it must not conflate them.
    pub measured: bool,
    /// One sentence naming how the answer was reached. Shown in small print
    /// under the status row, never in place of the status itself.
    pub detail: String,
}

/// Ask the system where permission stands.
///
/// Returns `Unknown` today. The `MEET_AI_FAKE_PERMISSION` override exists so
/// the denied path — the screen a user actually lands on after saying No — can
/// be built and reviewed before the capture layer can produce a real denial.
/// It is compiled out of release builds so it can never affect a shipped app.
pub fn status() -> Status {
    #[cfg(debug_assertions)]
    if let Ok(forced) = std::env::var("MEET_AI_FAKE_PERMISSION") {
        let state = match forced.as_str() {
            "granted" => State::Granted,
            "denied" => State::Denied,
            _ => State::Unknown,
        };
        return Status {
            state,
            measured: true,
            detail: format!(
                "Simulated by MEET_AI_FAKE_PERMISSION={forced}. This override only exists in \
                 development builds."
            ),
        };
    }

    Status {
        state: State::Unknown,
        measured: false,
        detail: "meet-ai cannot check this yet. The check plays a short tone and listens for it \
                 coming back, and that part of the recorder is still being built."
            .into(),
    }
}

/// System Settings panes worth deep-linking to.
///
/// SPEC §8.1 left the exact audio-capture anchor to be "verified at
/// implementation time, falling back to the pane root". Verified on macOS 27.0
/// (build 26A428) by reading the anchor table out of the Settings extension:
///
/// ```text
/// strings /System/Library/ExtensionKit/Extensions/SecurityPrivacyExtension.appex/\
///   Contents/MacOS/SecurityPrivacyExtension | grep -oE 'Privacy_[A-Za-z0-9]+'
/// ```
///
/// `Privacy_AudioCapture` and `Privacy_Microphone` are both in that table. The
/// pane root stays here as the third entry because an unknown anchor lands on
/// the root anyway, and because this list will be wrong on some future macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pane {
    /// "System Audio Recording Only" — the tap permission.
    AudioCapture,
    /// The microphone half, which is a separate grant.
    Microphone,
}

impl Pane {
    /// The `x-apple.systempreferences:` URL for this pane.
    pub fn url(self) -> &'static str {
        match self {
            Pane::AudioCapture => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
            }
            Pane::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
        }
    }
}

/// Where to send someone whose deep link did not land — the Privacy & Security
/// pane root. Used as the fallback, and named in the on-screen instructions so
/// the steps still work if the anchor stops resolving on a later macOS.
pub const PRIVACY_ROOT_URL: &str = "x-apple.systempreferences:com.apple.preference.security";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmeasured_permission_is_never_reported_as_granted() {
        // The default path must not flatter itself. Under the dev override the
        // test process might set the variable, so check the invariant that
        // actually matters: an unmeasured answer is never Granted.
        let status = status();
        if !status.measured {
            assert_eq!(status.state, State::Unknown);
        }
        assert!(
            !status.detail.is_empty(),
            "every status carries a sentence the UI can show"
        );
    }

    #[test]
    fn deep_links_use_anchors_verified_against_the_settings_extension() {
        assert!(Pane::AudioCapture.url().ends_with("?Privacy_AudioCapture"));
        assert!(Pane::Microphone.url().ends_with("?Privacy_Microphone"));
        for pane in [Pane::AudioCapture, Pane::Microphone] {
            assert!(
                pane.url().starts_with(PRIVACY_ROOT_URL),
                "every anchor must hang off the pane root, so the fallback is the same pane"
            );
        }
    }
}
