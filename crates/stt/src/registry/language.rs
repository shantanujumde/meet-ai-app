//! Whether the engine that will run honours `transcription.language`
//! (TUR-157), so Settings can disable the spoken-language picker and say why
//! instead of offering a choice that is quietly ignored.
//!
//! Only a multilingual whisper model is told the language
//! (`crate::platform::recording_config`). An English-only whisper model is
//! always told `en`, Apple's engine runs in the environment's locale, and
//! Parakeet works the language out on its own.

use super::{Environment, Kind, Selection};

/// Whether the spoken-language setting reaches the engine, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSupport {
    pub honours_language: bool,
    /// One plain sentence, set only when `honours_language` is false.
    pub reason: Option<String>,
}

/// Why the picker is off for an English-only whisper model.
pub const ENGLISH_ONLY_MODEL: &str =
    "This model only transcribes English. Pick a multilingual model to choose the language.";

/// Why the picker is off for Parakeet.
pub const PARAKEET_PICKS_ITS_OWN: &str =
    "Parakeet works out the language on its own and does not use this setting.";

impl LanguageSupport {
    fn yes() -> Self {
        Self {
            honours_language: true,
            reason: None,
        }
    }

    fn no(reason: impl Into<String>) -> Self {
        Self {
            honours_language: false,
            reason: Some(reason.into()),
        }
    }
}

/// Whether `engine`, run with `environment`, honours the spoken language.
///
/// For whisper the model decides: the file on disk when there is one, else
/// the catalogue entry config names. The same rule as
/// `crate::platform::recording_config`, so the picker cannot claim a setting
/// that recording then overrides.
pub fn language_support(engine: Kind, environment: &Environment) -> LanguageSupport {
    match engine {
        Kind::Whisper => {
            let own = match &environment.whisper_model {
                Some(path) => crate::model::whisper_language_for_file(path),
                None => crate::model::find(&environment.whisper_model_id)
                    .and_then(|spec| spec.whisper_language()),
            };
            match own {
                Some(_) => LanguageSupport::no(ENGLISH_ONLY_MODEL),
                None => LanguageSupport::yes(),
            }
        }
        Kind::AppleSpeech => LanguageSupport::no(format!(
            "Apple's speech engine transcribes in {} and does not use this setting.",
            environment.locale
        )),
        Kind::Parakeet => LanguageSupport::no(PARAKEET_PICKS_ITS_OWN),
    }
}

impl Selection {
    /// Whether the engine this selection picked honours the spoken language.
    pub fn language(&self, environment: &Environment) -> LanguageSupport {
        language_support(self.engine, environment)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn environment(model_id: &str, file: Option<&str>) -> Environment {
        Environment {
            sidecar: None,
            locale: "en-US".into(),
            whisper_model: file.map(PathBuf::from),
            whisper_model_id: model_id.into(),
            installed_whisper_models: Vec::new(),
            parakeet_model: None,
            spoken: Default::default(),
        }
    }

    #[test]
    fn a_multilingual_whisper_model_honours_the_language() {
        let env = environment(
            "large-v3-turbo-q5_0",
            Some("/m/ggml-large-v3-turbo-q5_0.bin"),
        );
        assert_eq!(
            language_support(Kind::Whisper, &env),
            LanguageSupport::yes()
        );
        // Not downloaded yet: the catalogue entry still answers.
        let env = environment("large-v3-turbo-q5_0", None);
        assert!(language_support(Kind::Whisper, &env).honours_language);
    }

    #[test]
    fn an_english_only_whisper_model_does_not_and_says_why() {
        for file in [Some("/m/ggml-small.en-q5_1.bin"), None] {
            let env = environment("small.en-q5_1", file);
            assert_eq!(
                language_support(Kind::Whisper, &env),
                LanguageSupport::no(ENGLISH_ONLY_MODEL),
                "{file:?}"
            );
        }
    }

    #[test]
    fn every_catalogue_model_agrees_with_what_recording_tells_whisper() {
        for spec in crate::model::MODELS {
            let path = PathBuf::from("/m").join(spec.filename);
            let env = environment(spec.id, Some(path.to_str().unwrap()));
            let config = crate::platform::recording_config(&path, crate::languages::spoken("mr"));
            assert_eq!(
                language_support(Kind::Whisper, &env).honours_language,
                config.language.as_deref() == Some("mr"),
                "{}",
                spec.id
            );
        }
    }

    #[test]
    fn parakeet_does_not_and_says_why() {
        let env = environment("large-v3-turbo-q5_0", None);
        assert_eq!(
            language_support(Kind::Parakeet, &env),
            LanguageSupport::no(PARAKEET_PICKS_ITS_OWN)
        );
    }

    #[test]
    fn apple_does_not_and_names_the_locale_it_uses() {
        let env = environment("large-v3-turbo-q5_0", None);
        let support = language_support(Kind::AppleSpeech, &env);
        assert!(!support.honours_language);
        assert_eq!(
            support.reason.as_deref(),
            Some("Apple's speech engine transcribes in en-US and does not use this setting.")
        );
    }

    #[test]
    fn a_selection_reports_it_for_the_engine_it_picked() {
        let env = environment("small.en-q5_1", None);
        let selection = Selection {
            engine: Kind::Parakeet,
            reason: String::new(),
        };
        assert_eq!(
            selection.language(&env),
            language_support(Kind::Parakeet, &env)
        );
    }
}
