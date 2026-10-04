//! The Settings engine picker (TUR-75): what each choice would do on this
//! Mac, and saving one into `config.jsonc`.
//!
//! Everything the picker greys out, and why, comes from
//! [`stt::registry::options`] — the same decision `auto` and recording use —
//! so the window never works out on its own which engine can run.
//!
//! A save takes effect on the next recording: `live_transcript` reads the
//! config when it opens an engine, and a recording already running keeps the
//! engine it opened with.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use stt::model::Recommendation;
use stt::registry::{self, Availability, Environment, Kind, Preference};

use crate::config::{self, Transcription};
use crate::error::UiError;

/// `transcription.engine`, as the window sends and receives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum EngineChoice {
    Auto,
    AppleSpeech,
    Whisper,
    Parakeet,
}

impl From<Preference> for EngineChoice {
    fn from(preference: Preference) -> Self {
        match preference {
            Preference::Auto => Self::Auto,
            Preference::AppleSpeech => Self::AppleSpeech,
            Preference::Whisper => Self::Whisper,
            Preference::Parakeet => Self::Parakeet,
        }
    }
}

impl From<EngineChoice> for Preference {
    fn from(choice: EngineChoice) -> Self {
        match choice {
            EngineChoice::Auto => Self::Auto,
            EngineChoice::AppleSpeech => Self::AppleSpeech,
            EngineChoice::Whisper => Self::Whisper,
            EngineChoice::Parakeet => Self::Parakeet,
        }
    }
}

/// An engine that actually runs: what "Automatic" lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedEngine {
    AppleSpeech,
    Whisper,
    Parakeet,
}

impl From<Kind> for ResolvedEngine {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::AppleSpeech => Self::AppleSpeech,
            Kind::Whisper => Self::Whisper,
            Kind::Parakeet => Self::Parakeet,
        }
    }
}

/// Whether one choice can be picked, and the sentence to show when not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineAvailability {
    pub available: bool,
    pub reason: Option<String>,
}

impl From<Availability> for EngineAvailability {
    fn from(availability: Availability) -> Self {
        Self {
            available: availability.available,
            reason: availability.reason,
        }
    }
}

/// Everything the picker draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineChoices {
    /// What `config.jsonc` says now.
    pub engine: EngineChoice,
    /// `transcription.model`: the whisper model in use when whisper runs.
    pub model: String,
    /// What "Automatic" resolves to on this Mac; `None` when nothing is ready.
    pub auto: Option<ResolvedEngine>,
    pub apple: EngineAvailability,
    pub whisper: EngineAvailability,
    /// Parakeet (TUR-62): pickable once its model folder is downloaded.
    pub parakeet: EngineAvailability,
    /// The Parakeet row's facts and whether its model is here.
    pub parakeet_model: super::ParakeetModelView,
    /// The locales Apple's engine has installed, e.g. `en-US`.
    pub languages: Vec<String>,
}

/// The picker as it stands. Runs the ~160 ms probe; call it off the render path.
pub fn choices() -> EngineChoices {
    let transcription = config::transcription();
    let environment = super::discover(super::DEFAULT_LOCALE, &transcription.model);
    view(&transcription, registry::options(&environment))
}

fn view(transcription: &Transcription, options: registry::EngineOptions) -> EngineChoices {
    let parakeet_model = super::parakeet::view(&super::ModelDirs::discover());
    EngineChoices {
        engine: transcription.engine.into(),
        model: transcription.model.clone(),
        auto: options.auto.map(Into::into),
        apple: options.apple.into(),
        whisper: options.whisper.into(),
        parakeet: options.parakeet.into(),
        parakeet_model,
        languages: options.installed_locales,
    }
}

/// Save `engine` and `model`, refusing a choice this Mac cannot run, and
/// return the picker as saved.
pub fn save_choice(engine: EngineChoice, model: &str) -> Result<EngineChoices, UiError> {
    let environment = super::discover(super::DEFAULT_LOCALE, model);
    let options = registry::options(&environment);
    check(engine, model, &environment, &options)?;
    config::set_transcription(engine.into(), model)?;
    let saved = Transcription {
        engine: engine.into(),
        model: model.to_string(),
    };
    Ok(view(&saved, options))
}

/// The same rules the picker draws, enforced again here: a disabled radio is
/// a hint, not a guarantee.
fn check(
    engine: EngineChoice,
    model: &str,
    environment: &Environment,
    options: &registry::EngineOptions,
) -> Result<(), UiError> {
    let spec = stt::model::find(model).ok_or_else(|| {
        UiError::app(
            "unknown-model",
            format!("meet-ai does not have a model called {model:?} in its list."),
        )
    })?;
    match engine {
        EngineChoice::AppleSpeech if !options.apple.available => Err(UiError::app(
            "engine-unavailable",
            options
                .apple
                .reason
                .clone()
                .unwrap_or_else(|| "Apple's engine can't run on this Mac.".into()),
        )),
        EngineChoice::Whisper if environment.whisper_model.is_none() => Err(UiError::app(
            "model-not-downloaded",
            format!(
                "{} isn't on this Mac yet. Download it first.",
                spec.facts.display_name
            ),
        )),
        // No ONNX Runtime in this build: a download would not help.
        EngineChoice::Parakeet
            if options.parakeet.reason.as_deref() == Some(registry::PARAKEET_NOT_IN_THIS_BUILD) =>
        {
            Err(UiError::app(
                "engine-unavailable",
                registry::PARAKEET_NOT_IN_THIS_BUILD,
            ))
        }
        EngineChoice::Parakeet if environment.parakeet_model.is_none() => Err(UiError::app(
            "model-not-downloaded",
            format!(
                "{} isn't downloaded yet. Download it first.",
                stt::model::parakeet::PARAKEET_V3.display_name
            ),
        )),
        _ => Ok(()),
    }
}

/// Which whisper model to mark "Recommended" on this machine, and the default
/// when config names none: the hardware tier (TUR-61). The GPU and the memory
/// do not change while the app runs, so they are read once. Off macOS that
/// asks ggml for its Vulkan devices, so the first call can take a moment.
pub(super) fn recommendation() -> Recommendation {
    static PICK: OnceLock<Recommendation> = OnceLock::new();
    PICK.get_or_init(|| {
        let app_dir = crate::meetings::root()
            .ok()
            .map(|root| meeting_format::layout::app_dir(&root));
        let hardware = stt::hardware::Hardware::detect(app_dir.as_deref());
        stt::model::recommended(&hardware)
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn environment(whisper_model: Option<&str>) -> Environment {
        Environment {
            sidecar: None,
            locale: "en-US".into(),
            whisper_model: whisper_model.map(PathBuf::from),
            whisper_model_id: "small.en-q5_1".into(),
            installed_whisper_models: whisper_model
                .map(|_| vec!["small.en-q5_1".into()])
                .unwrap_or_default(),
            parakeet_model: None,
        }
    }

    fn options(apple: bool, whisper: bool) -> registry::EngineOptions {
        let availability = |yes: bool, why: &str| Availability {
            available: yes,
            reason: (!yes).then(|| why.to_string()),
        };
        registry::EngineOptions {
            auto: Some(Kind::AppleSpeech),
            apple: availability(apple, "Needs macOS 26 or later."),
            whisper: availability(whisper, registry::WHISPER_NEEDS_A_MODEL),
            parakeet: availability(false, registry::PARAKEET_NEEDS_A_MODEL),
            installed_locales: vec!["en-US".into()],
        }
    }

    #[test]
    fn the_engine_choice_is_spelled_like_the_config() {
        for (choice, spelled) in [
            (EngineChoice::Auto, "\"auto\""),
            (EngineChoice::AppleSpeech, "\"apple-speech\""),
            (EngineChoice::Whisper, "\"whisper\""),
            (EngineChoice::Parakeet, "\"parakeet\""),
        ] {
            assert_eq!(serde_json::to_string(&choice).unwrap(), spelled);
            let preference: Preference = serde_json::from_str(spelled).unwrap();
            assert_eq!(EngineChoice::from(preference), choice);
            assert_eq!(Preference::from(choice), preference);
        }
    }

    #[test]
    fn the_view_carries_the_saved_choice_and_the_registry_answers() {
        let transcription = Transcription {
            engine: Preference::Whisper,
            model: "small.en-q5_1".into(),
        };
        let view = view(&transcription, options(true, false));
        assert_eq!(view.engine, EngineChoice::Whisper);
        assert_eq!(view.model, "small.en-q5_1");
        assert_eq!(view.auto, Some(ResolvedEngine::AppleSpeech));
        assert!(view.apple.available);
        assert_eq!(
            view.whisper.reason.as_deref(),
            Some(registry::WHISPER_NEEDS_A_MODEL)
        );
        assert_eq!(view.languages, vec!["en-US"]);
    }

    #[test]
    fn apple_is_refused_with_its_reason_when_it_cannot_run() {
        let error = check(
            EngineChoice::AppleSpeech,
            "small.en-q5_1",
            &environment(None),
            &options(false, false),
        )
        .unwrap_err();
        assert_eq!(error.kind, "engine-unavailable");
        assert_eq!(error.message, "Needs macOS 26 or later.");
    }

    #[test]
    fn whisper_is_refused_unless_the_picked_model_is_downloaded() {
        let error = check(
            EngineChoice::Whisper,
            "small.en-q5_1",
            &environment(None),
            &options(true, false),
        )
        .unwrap_err();
        assert_eq!(error.kind, "model-not-downloaded");
        assert!(
            error.message.contains("Small (English only)"),
            "{}",
            error.message
        );

        check(
            EngineChoice::Whisper,
            "small.en-q5_1",
            &environment(Some("/tmp/ggml-small.en-q5_1.bin")),
            &options(true, true),
        )
        .unwrap();
    }

    #[test]
    fn automatic_saves_with_any_known_model_but_not_an_unknown_one() {
        check(
            EngineChoice::Auto,
            "large-v3-turbo-q5_0",
            &environment(None),
            &options(false, false),
        )
        .unwrap();
        let error = check(
            EngineChoice::Auto,
            "tiny-made-up",
            &environment(None),
            &options(true, true),
        )
        .unwrap_err();
        assert_eq!(error.kind, "unknown-model");
    }

    #[test]
    fn this_mac_gets_a_recommendation_from_the_catalogue() {
        let pick = recommendation();
        assert!(stt::model::find(pick.model_id).is_some());
        assert!(!pick.reason.is_empty());
    }

    #[test]
    fn parakeet_is_refused_until_its_model_is_downloaded() {
        let error = check(
            EngineChoice::Parakeet,
            "small.en-q5_1",
            &environment(None),
            &options(true, true),
        )
        .unwrap_err();
        assert_eq!(error.kind, "model-not-downloaded");
        assert!(error.message.contains("Parakeet"), "{}", error.message);

        let mut ready = environment(None);
        ready.parakeet_model = Some(PathBuf::from("/tmp/parakeet-tdt-0.6b-v3-int8"));
        check(
            EngineChoice::Parakeet,
            "small.en-q5_1",
            &ready,
            &options(true, true),
        )
        .unwrap();
    }

    #[test]
    fn the_view_carries_parakeet_and_its_row() {
        let transcription = Transcription {
            engine: Preference::Parakeet,
            model: "small.en-q5_1".into(),
        };
        let view = view(&transcription, options(true, true));
        assert_eq!(view.engine, EngineChoice::Parakeet);
        assert_eq!(
            view.parakeet.reason.as_deref(),
            Some(registry::PARAKEET_NEEDS_A_MODEL)
        );
        assert_eq!(view.parakeet_model.id, "parakeet-tdt-0.6b-v3");
        assert_eq!(
            serde_json::to_string(&ResolvedEngine::Parakeet).unwrap(),
            "\"parakeet\""
        );
    }

    #[test]
    fn parakeet_without_onnx_runtime_is_refused_as_not_ready_here() {
        let mut no_runtime = options(true, true);
        no_runtime.parakeet = Availability {
            available: false,
            reason: Some(registry::PARAKEET_NOT_IN_THIS_BUILD.into()),
        };
        let mut ready = environment(None);
        ready.parakeet_model = Some(PathBuf::from("/tmp/parakeet-tdt-0.6b-v3-int8"));
        for env in [environment(None), ready] {
            let error =
                check(EngineChoice::Parakeet, "small.en-q5_1", &env, &no_runtime).unwrap_err();
            assert_eq!(error.kind, "engine-unavailable");
            assert_eq!(error.message, registry::PARAKEET_NOT_IN_THIS_BUILD);
        }
    }
}
