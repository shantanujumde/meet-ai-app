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
    /// `transcription.language`: `auto`, or the whisper code people speak.
    pub spoken_language: String,
    /// What the spoken-language picker offers besides `auto`, in its order:
    /// Hinglish, then every language whisper has a token for, by name.
    pub spoken_languages: Vec<SpokenLanguageOption>,
    /// What in `transcription` was not valid and is shown as its default
    /// (TUR-155), such as `"engine": "whispr"`; `None` when all of it was.
    pub config_problem: Option<String>,
    /// Whether the engine that will run (and, for whisper, its model) uses
    /// `spoken_language` (TUR-157). When false the picker is disabled and
    /// `language_ignored_reason` says why; the saved value is kept.
    pub honours_language: bool,
    pub language_ignored_reason: Option<String>,
    /// Why the saved engine cannot run here, from the same probe as the rest
    /// (TUR-171); `None` when it can. Settings used to ask `engine_selection`
    /// for this, which ran the probe a second time.
    pub selection_error: Option<UiError>,
}

/// One language the spoken-language picker offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpokenLanguageOption {
    /// The whisper code, such as `mr`.
    pub code: String,
    /// The English name, such as `Marathi`.
    pub name: String,
}

/// Hinglish first, since it is not one of whisper's own, then whisper's
/// languages by name.
fn spoken_languages() -> Vec<SpokenLanguageOption> {
    let mut languages: Vec<_> = stt::languages::WHISPER_LANGUAGES.to_vec();
    languages.sort_by_key(|(_, name)| *name);
    std::iter::once((stt::languages::HINGLISH, HINGLISH_NAME))
        .chain(languages)
        .map(|(code, name)| SpokenLanguageOption {
            code: code.to_string(),
            name: name.to_string(),
        })
        .collect()
}

const HINGLISH_NAME: &str = "Hinglish (Hindi and English)";

/// The picker as it stands. Runs the ~160 ms probe; call it off the render path.
pub fn choices() -> EngineChoices {
    let config::Checked {
        value: transcription,
        problem,
    } = config::transcription_checked();
    let environment = super::discover(super::DEFAULT_LOCALE, &transcription.model);
    let (options, selected) = registry::options_and_resolve(transcription.engine, &environment);
    EngineChoices {
        config_problem: problem,
        selection_error: selected.err().map(UiError::from),
        ..view(&transcription, options, &environment)
    }
}

fn view(
    transcription: &Transcription,
    options: registry::EngineOptions,
    environment: &Environment,
) -> EngineChoices {
    let parakeet_model = super::parakeet::view(&super::ModelDirs::discover());
    let language = language_support(transcription.engine, options.auto, environment);
    EngineChoices {
        engine: transcription.engine.into(),
        model: transcription.model.clone(),
        auto: options.auto.map(Into::into),
        apple: options.apple.into(),
        whisper: options.whisper.into(),
        parakeet: options.parakeet.into(),
        parakeet_model,
        languages: options.installed_locales,
        spoken_language: transcription.language.clone(),
        spoken_languages: spoken_languages(),
        config_problem: None,
        honours_language: language.honours_language,
        language_ignored_reason: language.reason,
        selection_error: None,
    }
}

/// [`registry::language_support`] for the saved engine, with `auto` read as
/// what it resolves to here. Nothing ready yet leaves the picker on.
fn language_support(
    engine: Preference,
    auto: Option<Kind>,
    environment: &Environment,
) -> registry::LanguageSupport {
    let kind = match engine {
        Preference::Auto => auto,
        Preference::AppleSpeech => Some(Kind::AppleSpeech),
        Preference::Whisper => Some(Kind::Whisper),
        Preference::Parakeet => Some(Kind::Parakeet),
    };
    match kind {
        Some(kind) => registry::language_support(kind, environment),
        None => registry::LanguageSupport {
            honours_language: true,
            reason: None,
        },
    }
}

/// Save `engine` and `model`, refusing a choice this Mac cannot run, and
/// return the picker as saved.
pub fn save_choice(engine: EngineChoice, model: &str) -> Result<EngineChoices, UiError> {
    let environment = super::discover(super::DEFAULT_LOCALE, model);
    let (options, selected) = registry::options_and_resolve(engine.into(), &environment);
    check(engine, model, &environment, &options)?;
    config::set_transcription(engine.into(), model)?;
    let saved = Transcription {
        engine: engine.into(),
        model: model.to_string(),
        ..config::transcription()
    };
    Ok(EngineChoices {
        selection_error: selected.err().map(UiError::from),
        ..view(&saved, options, &environment)
    })
}

/// Save `transcription.language`: `auto` or a whisper code. Anything else is
/// refused rather than written, since the reader would quietly treat it as
/// auto. Returns the picker as saved.
pub fn save_language(language: &str) -> Result<EngineChoices, UiError> {
    let language = language.trim();
    let known = language == stt::languages::AUTO
        || language == stt::languages::HINGLISH
        || stt::languages::whisper_code(language).is_some();
    if !known {
        return Err(UiError::from(config::ConfigError::Invalid(format!(
            "transcription.language {language:?} is not a language whisper knows"
        ))));
    }
    config::set_transcription_language(language)?;
    Ok(choices())
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
            spoken: Default::default(),
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
            language: "auto".into(),
            live: true,
        };
        let view = view(&transcription, options(true, false), &environment(None));
        assert_eq!(view.spoken_language, "auto");
        assert_eq!(view.spoken_languages.len(), 101);
        assert_eq!(view.spoken_languages[0].code, "hinglish");
        assert_eq!(view.spoken_languages[1].name, "Afrikaans");
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
    fn the_view_says_whether_the_saved_engine_uses_the_spoken_language() {
        let saved = |engine, model: &str| Transcription {
            engine,
            model: model.into(),
            language: "mr".into(),
            live: true,
        };
        let mut multilingual = environment(Some("/tmp/ggml-large-v3-turbo-q5_0.bin"));
        multilingual.whisper_model_id = "large-v3-turbo-q5_0".into();

        let turbo = view(
            &saved(Preference::Whisper, "large-v3-turbo-q5_0"),
            options(true, true),
            &multilingual,
        );
        assert!(turbo.honours_language);
        assert_eq!(turbo.language_ignored_reason, None);

        let english = view(
            &saved(Preference::Whisper, "small.en-q5_1"),
            options(true, true),
            &environment(Some("/tmp/ggml-small.en-q5_1.bin")),
        );
        assert!(!english.honours_language);
        assert_eq!(
            english.language_ignored_reason.as_deref(),
            Some(registry::ENGLISH_ONLY_MODEL)
        );
        // The saved language is kept, not overwritten.
        assert_eq!(english.spoken_language, "mr");

        // Automatic is read as what it lands on: Apple's engine here.
        let auto = view(
            &saved(Preference::Auto, "large-v3-turbo-q5_0"),
            options(true, true),
            &multilingual,
        );
        assert!(!auto.honours_language);
        assert!(
            auto.language_ignored_reason
                .as_deref()
                .is_some_and(|why| why.contains("en-US")),
            "{:?}",
            auto.language_ignored_reason
        );

        let parakeet = view(
            &saved(Preference::Parakeet, "large-v3-turbo-q5_0"),
            options(true, true),
            &multilingual,
        );
        assert_eq!(
            parakeet.language_ignored_reason.as_deref(),
            Some(registry::PARAKEET_PICKS_ITS_OWN)
        );

        let mut nothing_ready = options(false, false);
        nothing_ready.auto = None;
        let none = view(
            &saved(Preference::Auto, "large-v3-turbo-q5_0"),
            nothing_ready,
            &environment(None),
        );
        assert!(none.honours_language);
    }

    #[test]
    fn a_language_whisper_does_not_know_is_refused_not_written() {
        for bad in ["marathi", "xx", "en-US"] {
            assert!(save_language(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_view_carries_parakeet_and_its_row() {
        let transcription = Transcription {
            engine: Preference::Parakeet,
            model: "small.en-q5_1".into(),
            language: "auto".into(),
            live: true,
        };
        let view = view(&transcription, options(true, true), &environment(None));
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
