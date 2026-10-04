//! Audio-capture permission, as the onboarding flow sees it.
//!
//! **This module owns the screen, not the measurement.** The measurement is
//! [`audio::permission_check`]. SPEC §8.1 is blunt about why it is hard: on
//! denial *every* `OSStatus` is `noErr` and the process tap delivers bit-exact
//! zeros, which is indistinguishable from a granted capture of a silent Mac.
//! So a return code is not evidence and an RMS floor is not evidence. The only
//! proof is a positive control — play a ~200 ms known tone from meet-ai's own
//! process and confirm it comes back through the tap.
//!
//! [`status`] never measures anything — it is the fast, always-safe default,
//! and the thing `cargo test` calls, so a test run never touches real audio
//! hardware. [`measure`] is the real, on-demand check: it takes real
//! wall-clock time and must be run off the UI thread (see
//! `commands::measure_permission`). Neither reports `Granted`
//! optimistically: an onboarding screen that claims permission it has not
//! measured is worse than one that admits it does not know, because the user
//! only finds out when a real meeting records silence.

use audio::permission_check::{self, ChannelResult, ChannelState};
use serde::{Deserialize, Serialize};

/// Where the user stands with audio permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
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
#[derive(Debug, Clone, Serialize, specta::Type)]
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
    /// Which grants are off, as the System Settings pane each one lives in, so
    /// the screen can name the switch to flip rather than "audio" in general.
    /// Empty unless `state` is `Denied`.
    pub denied: Vec<Pane>,
}

impl Status {
    /// Whether this answer refuses a recording: only a denied microphone
    /// does (TUR-87). With only System Audio Recording off the microphone
    /// still works, and a microphone-only recording is the documented
    /// degraded mode (contract §9), so that records instead of refusing.
    pub fn blocks_recording(&self) -> bool {
        self.state == State::Denied && !self.system_audio_off()
    }

    /// Whether system audio, and only system audio, is denied: the
    /// recording then starts microphone-only and the window shows the
    /// "System audio is off" banner.
    pub fn system_audio_off(&self) -> bool {
        self.state == State::Denied && self.denied == [Pane::AudioCapture]
    }

    /// The system-audio source a recording under this answer should use:
    /// none when system audio is denied (a tap would only deliver zeros),
    /// otherwise the platform's default.
    pub fn system_source(&self) -> Option<Box<dyn audio::AudioSource>> {
        if self.system_audio_off() {
            tracing::warn!("system audio is denied; recording the microphone only");
            return None;
        }
        audio::session::default_system_source()
    }

    /// Why a recording was refused, naming the switch that is off.
    pub fn refusal_message(&self) -> String {
        let names: Vec<&str> = self.denied.iter().map(|pane| pane.label()).collect();
        match names.as_slice() {
            [] => "meet-ai did not start recording: it is not allowed to record this Mac's audio."
                .to_string(),
            [one] => format!(
                "meet-ai did not start recording: {one} is switched off for meet-ai in System \
                 Settings, so it cannot record you."
            ),
            _ => format!(
                "meet-ai did not start recording: {} are switched off for meet-ai in System \
                 Settings, so it cannot record you.",
                names.join(" and ")
            ),
        }
    }
}

/// The fast, always-safe default: never measured, never `Granted`.
///
/// Used as the pre-check placeholder and as the fallback if [`measure`]'s
/// blocking task itself cannot be joined. The `MEET_AI_FAKE_PERMISSION`
/// override exists so the denied path — the screen a user actually lands on
/// after saying No — can be built and reviewed without needing a real denial
/// on hand. It is compiled out of release builds so it can never affect a
/// shipped app.
pub fn status() -> Status {
    #[cfg(debug_assertions)]
    if let Some(forced) = forced_status() {
        return forced;
    }

    Status {
        state: State::Unknown,
        measured: false,
        detail: "meet-ai has not checked audio permission yet.".into(),
        denied: Vec::new(),
    }
}

/// Run the real positive-control measurement (SPEC §8.1/A6): a start/stop
/// probe against the microphone, and the permission chime played through the
/// default output device and listened for on the system-audio tap.
///
/// Takes real wall-clock time — at least
/// [`audio::chime::ONSET_TIMEOUT_MILLIS`] for the system-audio half alone —
/// and must be run off the UI thread. See `commands::measure_permission`.
pub fn measure() -> Status {
    #[cfg(debug_assertions)]
    if let Some(forced) = forced_status() {
        return forced;
    }

    // TUR-91: the mic this check opens is the one the recording after it
    // opens, so both follow the setting as `config.jsonc` has it now.
    crate::mic_setting::apply();
    combine(
        permission_check::check_mic(),
        permission_check::check_system(),
    )
}

/// What can be known without making a sound: the microphone's stored decision.
///
/// The app runs this every time it starts, where [`measure`]'s chime would
/// beep on every launch — TUR-24 and SPEC A7 keep the chime to setup and the
/// start of a recording. It only ever answers `Denied` or `Unknown`: system
/// audio has no silent answer, so a switched-off system-audio grant is caught
/// by the next Record press, which runs [`measure`] and refuses.
pub fn quick() -> Status {
    #[cfg(debug_assertions)]
    if let Some(forced) = forced_status() {
        return forced;
    }

    let mic = permission_check::mic_decision();
    if mic.state == ChannelState::Denied {
        return Status {
            state: State::Denied,
            measured: true,
            detail: format!(
                "microphone: {}. system audio: checked when you next record.",
                mic.detail
            ),
            denied: vec![Pane::Microphone],
        };
    }
    Status {
        state: State::Unknown,
        measured: false,
        detail: "meet-ai checks audio permission when you start a recording.".into(),
        denied: Vec::new(),
    }
}

/// Fold the two channel readings into one [`Status`].
///
/// Denied wins over everything else — if either channel is definitely off,
/// the meeting will be half-recorded regardless of what the other channel
/// says, and the screen has to name that switch. Only a denied microphone
/// refuses a recording, though ([`Status::blocks_recording`]). Granted requires *both* channels to have measured cleanly; anything
/// else (a device missing, a read failing) is `Unknown`, never a guess in
/// either direction, because guessing wrong sends someone to instructions
/// that cannot help them (a false denial) or lets a broken check pass silently
/// (a false grant).
fn combine(mic: ChannelResult, system: ChannelResult) -> Status {
    let state = if mic.state == ChannelState::Denied || system.state == ChannelState::Denied {
        State::Denied
    } else if mic.state == ChannelState::Granted && system.state == ChannelState::Granted {
        State::Granted
    } else {
        State::Unknown
    };

    let detail = match state {
        State::Granted => "meet-ai played a short tone and confirmed it can hear both the \
                            microphone and system audio."
            .to_string(),
        State::Denied => format!(
            "microphone: {}. system audio: {}.",
            mic.detail, system.detail
        ),
        State::Unknown => format!(
            "meet-ai could not finish checking. microphone: {}. system audio: {}.",
            mic.detail, system.detail
        ),
    };

    let mut denied = Vec::new();
    if mic.state == ChannelState::Denied {
        denied.push(Pane::Microphone);
    }
    if system.state == ChannelState::Denied {
        denied.push(Pane::AudioCapture);
    }

    Status {
        state,
        measured: true,
        detail,
        denied,
    }
}

#[cfg(debug_assertions)]
fn forced_status() -> Option<Status> {
    let forced = std::env::var("MEET_AI_FAKE_PERMISSION").ok()?;
    let state = match forced.as_str() {
        "granted" => State::Granted,
        "denied" => State::Denied,
        _ => State::Unknown,
    };
    Some(Status {
        state,
        measured: true,
        detail: format!(
            "Simulated by MEET_AI_FAKE_PERMISSION={forced}. This override only exists in \
             development builds."
        ),
        denied: if state == State::Denied {
            vec![Pane::Microphone, Pane::AudioCapture]
        } else {
            Vec::new()
        },
    })
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
/// `Privacy_AudioCapture`, `Privacy_Microphone` and `Privacy_Calendars` are
/// all in that table. The pane root stays as the fallback because an unknown
/// anchor lands on the root anyway, and because this list will be wrong on
/// some future macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pane {
    /// "System Audio Recording Only" — the tap permission.
    AudioCapture,
    /// The microphone half, which is a separate grant.
    Microphone,
    /// Calendar access, for the Today pane (TUR-28). Never in an audio
    /// [`Status::denied`] list.
    Calendars,
}

impl Pane {
    /// The switch's name as System Settings shows it.
    pub fn label(self) -> &'static str {
        match self {
            Pane::AudioCapture => "System Audio Recording",
            Pane::Microphone => "Microphone",
            Pane::Calendars => "Calendars",
        }
    }

    /// The `x-apple.systempreferences:` URL for this pane.
    pub fn url(self) -> &'static str {
        match self {
            Pane::AudioCapture => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture"
            }
            Pane::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            Pane::Calendars => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars"
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

    fn reading(state: ChannelState) -> ChannelResult {
        ChannelResult {
            state,
            detail: "test reading".into(),
        }
    }

    #[test]
    fn either_channel_denied_reports_denied_even_if_the_other_is_granted() {
        // A half-recorded meeting is what this guards: one grant is not enough.
        let status = combine(
            reading(ChannelState::Denied),
            reading(ChannelState::Granted),
        );
        assert_eq!(status.state, State::Denied);
        assert!(status.measured);

        let status = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Denied),
        );
        assert_eq!(status.state, State::Denied);
    }

    #[test]
    fn a_denial_names_the_switch_that_is_off() {
        // "not allowed to record" alone does not say where to go: the two
        // grants live in different Settings panes (TUR-127 review).
        let status = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Denied),
        );
        assert_eq!(status.denied, vec![Pane::AudioCapture]);
        let refusal = status.refusal_message();
        assert!(refusal.contains("System Audio Recording"), "{refusal}");
        assert!(!refusal.contains("Microphone"), "{refusal}");

        let status = combine(reading(ChannelState::Denied), reading(ChannelState::Denied));
        assert_eq!(status.denied, vec![Pane::Microphone, Pane::AudioCapture]);
        let refusal = status.refusal_message();
        assert!(
            refusal.contains("Microphone and System Audio Recording are"),
            "{refusal}"
        );

        let status = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Granted),
        );
        assert!(status.denied.is_empty());
    }

    #[test]
    fn only_a_denied_microphone_blocks_recording() {
        // TUR-87 H2: system audio alone denied still records the microphone.
        let system_only = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Denied),
        );
        assert_eq!(system_only.state, State::Denied, "the screen still says so");
        assert!(system_only.system_audio_off());
        assert!(!system_only.blocks_recording());
        assert!(system_only.system_source().is_none(), "mic-only recording");

        let mic_only = combine(
            reading(ChannelState::Denied),
            reading(ChannelState::Granted),
        );
        assert!(mic_only.blocks_recording());
        assert!(!mic_only.system_audio_off());

        let both = combine(reading(ChannelState::Denied), reading(ChannelState::Denied));
        assert!(both.blocks_recording());

        let unknown = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Unmeasurable),
        );
        assert!(!unknown.blocks_recording() && !unknown.system_audio_off());
    }

    #[test]
    fn a_refusal_never_claims_the_recording_would_be_silent() {
        let status = combine(
            reading(ChannelState::Denied),
            reading(ChannelState::Granted),
        );
        let refusal = status.refusal_message();
        assert!(!refusal.contains("silence"), "{refusal}");
        assert!(refusal.contains("Microphone"), "{refusal}");
    }

    #[test]
    fn both_channels_granted_is_the_only_path_to_granted() {
        let status = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Granted),
        );
        assert_eq!(status.state, State::Granted);
        assert!(status.measured);
    }

    #[test]
    fn an_unmeasurable_channel_never_reports_granted_or_denied() {
        // No output device, a read failure, etc. — the check simply did not
        // run, which is not evidence of a grant or a denial either way.
        let status = combine(
            reading(ChannelState::Granted),
            reading(ChannelState::Unmeasurable),
        );
        assert_eq!(status.state, State::Unknown);
        assert!(status.measured, "an attempt was still made");

        let status = combine(
            reading(ChannelState::Unmeasurable),
            reading(ChannelState::Unmeasurable),
        );
        assert_eq!(status.state, State::Unknown);
    }

    #[test]
    fn deep_links_use_anchors_verified_against_the_settings_extension() {
        assert!(Pane::AudioCapture.url().ends_with("?Privacy_AudioCapture"));
        assert!(Pane::Microphone.url().ends_with("?Privacy_Microphone"));
        assert!(Pane::Calendars.url().ends_with("?Privacy_Calendars"));
        for pane in [Pane::AudioCapture, Pane::Microphone, Pane::Calendars] {
            assert!(
                pane.url().starts_with(PRIVACY_ROOT_URL),
                "every anchor must hang off the pane root, so the fallback is the same pane"
            );
        }
    }
}
