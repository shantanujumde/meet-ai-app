//! What each choice in the Settings engine picker would do on this Mac
//! (TUR-75): which engine "Automatic" lands on, and whether Apple's engine
//! and whisper can be picked at all, with the reason when not.
//!
//! Lives in the registry because the registry is what decides `auto`. The
//! window never re-derives any of this: it draws what [`options`] says.

use super::{AppleUnavailable, Environment, Kind, Preference, decide, probe_apple};
use super::{parakeet, parakeet_runtime_missing};
use crate::Error;
use crate::apple::Probe;

/// Whether one engine can be picked, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Availability {
    pub available: bool,
    /// One plain sentence, set only when `available` is false.
    pub reason: Option<String>,
}

impl Availability {
    fn yes() -> Self {
        Self {
            available: true,
            reason: None,
        }
    }

    fn no(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
        }
    }
}

/// The picker's facts, from one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOptions {
    /// The engine `auto` resolves to here, or `None` when nothing is ready.
    pub auto: Option<Kind>,
    pub apple: Availability,
    pub whisper: Availability,
    /// Parakeet (TUR-62): pickable once its model folder is downloaded.
    pub parakeet: Availability,
    /// Locales whose on-device model Apple's engine has installed, as BCP 47
    /// ids. Empty when the probe did not run or did not say.
    pub installed_locales: Vec<String>,
}

/// Why whisper cannot be picked: no model on disk.
pub const WHISPER_NEEDS_A_MODEL: &str = "Download a model first.";

/// Why Parakeet cannot be picked: its model is not on disk.
pub const PARAKEET_NEEDS_A_MODEL: &str = "Download the Parakeet model first.";

/// Probe once, then answer for every choice.
pub fn options(environment: &Environment) -> EngineOptions {
    options_with(environment, &probe_apple(environment))
}

/// [`options`], and [`super::resolve`] for `preference`, from one probe
/// (TUR-171). Settings needs both: the picker, and whether the saved engine
/// can run. Asking for them apart ran `meet-stt --probe` twice.
pub fn options_and_resolve(
    preference: Preference,
    environment: &Environment,
) -> (EngineOptions, Result<super::Selection, Error>) {
    let apple = probe_apple(environment);
    (
        options_with(environment, &apple),
        decide(preference, environment, &apple),
    )
}

fn options_with(environment: &Environment, apple: &Option<Result<Probe, Error>>) -> EngineOptions {
    let auto = decide(Preference::Auto, environment, apple)
        .ok()
        .map(|selection| selection.engine);
    let apple_availability = if decide(Preference::AppleSpeech, environment, apple).is_ok() {
        Availability::yes()
    } else {
        Availability::no(apple_reason(apple, &environment.locale))
    };
    // Any model will do: picking whisper also picks one of the downloaded
    // models, so "the configured one is missing" is not a reason to refuse.
    let whisper = if environment.installed_whisper_models.is_empty() {
        Availability::no(WHISPER_NEEDS_A_MODEL)
    } else {
        Availability::yes()
    };
    let parakeet = parakeet::availability(environment, parakeet_runtime_missing().is_some());
    let installed_locales = match apple {
        Some(Ok(probe)) => probe.installed_locales.clone(),
        _ => Vec::new(),
    };
    EngineOptions {
        auto,
        apple: apple_availability,
        whisper,
        parakeet,
        installed_locales,
    }
}

/// [`AppleUnavailable`]'s cases, in words for the picker's disabled row
/// rather than for a log ([`super::apple_unavailable_detail`]).
fn apple_reason(apple: &Option<Result<Probe, Error>>, locale: &str) -> String {
    match AppleUnavailable::of(apple) {
        // Off macOS there is no helper to be missing (TUR-52).
        AppleUnavailable::NoSidecar if crate::platform::APPLE_SPEECH_UNSUPPORTED.is_some() => {
            "Only available on macOS.".into()
        }
        AppleUnavailable::NoSidecar => {
            "This copy of meet-ai has no speech helper, so Apple's engine can't run.".into()
        }
        AppleUnavailable::ProbeFailed(error) => {
            format!("Apple's engine did not answer when checked ({error}).")
        }
        AppleUnavailable::TooOld(_) => "Needs macOS 26 or later.".into(),
        AppleUnavailable::CannotRun => "Apple's engine says it can't run on this Mac (usually \
                                        the hardware or the language)."
            .into(),
        AppleUnavailable::ModelMissing => {
            format!("The on-device speech model for {locale} isn't installed on this Mac yet.")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn environment() -> Environment {
        Environment {
            sidecar: None,
            locale: "en-US".into(),
            whisper_model: None,
            whisper_model_id: "large-v3-turbo-q5_0".into(),
            installed_whisper_models: Vec::new(),
            parakeet_model: None,
            spoken: Default::default(),
        }
    }

    fn probe(available: bool, installed: bool, reason: Option<&str>) -> Probe {
        Probe {
            available,
            installed,
            locale: Some("en-US".into()),
            reason: reason.map(Into::into),
            os_version: None,
            installed_locales: if installed {
                vec!["en-US".into(), "fr-FR".into()]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn a_ready_apple_engine_is_what_auto_uses_and_lists_its_languages() {
        let found = options_with(&environment(), &Some(Ok(probe(true, true, None))));
        assert_eq!(found.auto, Some(Kind::AppleSpeech));
        assert_eq!(found.apple, Availability::yes());
        assert_eq!(found.installed_locales, vec!["en-US", "fr-FR"]);
    }

    #[test]
    fn whisper_is_refused_until_a_model_is_downloaded() {
        let found = options_with(&environment(), &Some(Ok(probe(true, true, None))));
        assert_eq!(found.whisper, Availability::no(WHISPER_NEEDS_A_MODEL));

        let mut env = environment();
        env.installed_whisper_models = vec!["small.en-q5_1".into()];
        let found = options_with(&env, &Some(Ok(probe(true, true, None))));
        assert_eq!(found.whisper, Availability::yes());
    }

    #[test]
    fn an_old_mac_says_it_needs_macos_26() {
        let apple = Some(Ok(probe(
            false,
            false,
            Some("SpeechTranscriber needs macOS 26 or later"),
        )));
        let found = options_with(&environment(), &apple);
        assert_eq!(found.apple, Availability::no("Needs macOS 26 or later."));
        assert_eq!(found.auto, None, "no model either, so nothing is ready");
    }

    #[test]
    fn auto_lands_on_whisper_when_apple_cannot_run_and_the_model_is_there() {
        let mut env = environment();
        env.whisper_model = Some(PathBuf::from("/tmp/ggml-large-v3-turbo-q5_0.bin"));
        env.installed_whisper_models = vec!["large-v3-turbo-q5_0".into()];
        let found = options_with(&env, &None);
        assert_eq!(found.auto, Some(Kind::Whisper));
        assert!(!found.apple.available);
        assert!(
            found.apple.reason.as_deref().unwrap().contains(
                if crate::platform::APPLE_SPEECH_UNSUPPORTED.is_some() {
                    "Only available on macOS"
                } else {
                    "speech helper"
                }
            ),
            "{:?}",
            found.apple.reason
        );
        assert!(found.installed_locales.is_empty());
    }

    #[test]
    fn a_missing_locale_model_and_an_unsupported_mac_are_told_apart() {
        let found = options_with(&environment(), &Some(Ok(probe(true, false, None))));
        assert_eq!(
            found.apple.reason.as_deref(),
            Some("The on-device speech model for en-US isn't installed on this Mac yet.")
        );
        let found = options_with(&environment(), &Some(Ok(probe(false, false, None))));
        assert!(
            found.apple.reason.as_deref().unwrap().contains("hardware"),
            "{:?}",
            found.apple.reason
        );
    }

    #[test]
    fn the_picker_agrees_with_resolve_about_auto() {
        // Same decision function, so the picker can never claim one engine
        // while recording opens another.
        let env = environment();
        let apple = Some(Ok(probe(true, true, None)));
        assert_eq!(
            options_with(&env, &apple).auto,
            decide(Preference::Auto, &env, &apple)
                .ok()
                .map(|s| s.engine)
        );
    }

    #[test]
    fn options_and_resolve_answer_like_the_two_calls_they_replace() {
        // No sidecar, so no probe runs; the answers must match asking apart.
        let mut env = environment();
        for preference in [
            Preference::AppleSpeech,
            Preference::Whisper,
            Preference::Auto,
        ] {
            let (found, resolved) = options_and_resolve(preference, &env);
            assert_eq!(found, options(&env));
            assert_eq!(
                resolved.map_err(|error| error.to_string()),
                super::super::resolve(preference, &env).map_err(|error| error.to_string())
            );
        }
        env.whisper_model = Some(PathBuf::from("/tmp/ggml-small.en-q5_1.bin"));
        let (_, resolved) = options_and_resolve(Preference::Whisper, &env);
        assert_eq!(resolved.unwrap().engine, Kind::Whisper);
    }
}
