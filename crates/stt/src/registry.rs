//! Runtime engine selection.
//!
//! SPEC §2.5: "the app never branches on platform outside the engine
//! registry". This module is that registry. It is the only place in the
//! codebase allowed to know that Apple's engine and whisper are different
//! things; everywhere else holds a `Box<dyn SttEngine>`.
//!
//! The Phase 1 exit gate says the engine switch must be a config change only.
//! That is enforced here by [`select`] taking a [`Preference`] — which is the
//! deserialized `transcription.engine` field and nothing else — and returning a
//! trait object.

use std::path::{Path, PathBuf};

use crate::apple::{AppleEngine, Probe};
use crate::{Error, SttEngine};

// The Settings picker's view of every choice at once (TUR-75).
mod options;
pub use options::{Availability, EngineOptions, WHISPER_NEEDS_A_MODEL, options};

// Selection *logic* is not platform-specific and compiles everywhere. What
// differs per OS (whether Apple's engine can exist at all, and how whisper is
// built) lives in `crate::platform`, the only module that names an OS.

/// Which engine the user asked for, from `config.jsonc`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preference {
    /// Use the best engine this Mac can offer. The default, and what the
    /// defaults table in SPEC §2.5 describes.
    #[default]
    Auto,
    /// Force Apple's `SpeechTranscriber`. Errors if unavailable rather than
    /// silently falling back — if you asked for it, you want to know.
    AppleSpeech,
    /// Force `whisper-rs`, even on macOS 26+. Useful for the Phase 1 gate,
    /// which has to read the same recording on both engines.
    Whisper,
}

/// Which engine actually got picked, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub engine: Kind,
    /// A sentence fit for a log line or the UI's about screen.
    pub reason: String,
}

/// The canonical engine names.
///
/// Defined here rather than on the engine types because [`Kind`] has to name
/// every engine on every platform, while `AppleEngine` only works on macOS.
/// The engines reference these back, so there is still one source of
/// truth — see `AppleEngine::NAME` and `WhisperEngine::NAME`.
pub const APPLE_SPEECH: &str = "apple-speech";
pub const WHISPER: &str = "whisper";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    AppleSpeech,
    Whisper,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::AppleSpeech => APPLE_SPEECH,
            Kind::Whisper => WHISPER,
        }
    }
}

/// Everything selection needs to know about this machine's setup.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Path to the `meet-stt` sidecar, if it was found.
    pub sidecar: Option<PathBuf>,
    /// Locale to transcribe in, e.g. `en-US`.
    pub locale: String,
    /// Path to a downloaded whisper model, if one is present.
    pub whisper_model: Option<PathBuf>,
    /// The whisper model id config asked for (`transcription.model`), so an
    /// error can name it.
    pub whisper_model_id: String,
    /// Ids of every catalogue model that is on disk, the wanted one or not.
    /// Only used to word errors: "model X is not downloaded (installed: Y)"
    /// instead of "no model is downloaded" when Y is right there.
    pub installed_whisper_models: Vec<String>,
}

impl Environment {
    /// Discover the environment from the filesystem, looking for models under
    /// the fallback root ([`crate::model::default_model_dir`]).
    ///
    /// For callers with no app root to hand — the CLI and the examples. The
    /// app knows where the user put their meetings and calls
    /// [`Environment::discover_in`] with that instead, so a model that moved
    /// with the meetings folder is still found.
    pub fn discover(locale: &str, model_id: &str) -> Self {
        let dir = crate::model::default_model_dir().ok();
        Self::discover_in(dir.as_deref(), locale, model_id)
    }

    /// Discover the environment from the filesystem, looking for models in
    /// `models_dir` (`None` when the caller could not work one out, which reads
    /// as "no whisper model", not as an error).
    ///
    /// Deliberately does not download anything: this answers "what can we do
    /// right now, offline", which is the question selection needs.
    pub fn discover_in(models_dir: Option<&Path>, locale: &str, model_id: &str) -> Self {
        let whisper_model = models_dir
            .and_then(|dir| crate::model::find(model_id).map(|spec| dir.join(spec.filename)))
            .filter(|path| path.is_file());
        let installed_whisper_models = models_dir
            .map(|dir| {
                crate::model::MODELS
                    .iter()
                    .filter(|spec| crate::model::is_installed(spec, dir))
                    .map(|spec| spec.id.to_string())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            sidecar: AppleEngine::discover(),
            locale: locale.to_string(),
            whisper_model,
            whisper_model_id: model_id.to_string(),
            installed_whisper_models,
        }
    }

    /// "model large-v3-turbo-q5_0 is not downloaded (installed: small.en-q5_1)",
    /// or "... (none installed)". Says which model is missing, so a user with a
    /// different one on disk is not told nothing is downloaded (TUR-23).
    fn missing_whisper_model(&self) -> String {
        let installed = if self.installed_whisper_models.is_empty() {
            "none installed".to_string()
        } else {
            format!("installed: {}", self.installed_whisper_models.join(", "))
        };
        format!(
            "model {} is not downloaded ({installed})",
            self.whisper_model_id
        )
    }
}

/// Which of the four ways Apple's engine can be unavailable happened. The one
/// place that reads a probe for it: [`apple_unavailable_detail`] words it for
/// a log or an error, `options::apple_reason` for the picker.
#[derive(Debug, Clone, Copy)]
enum AppleUnavailable<'a> {
    /// This build was not given the sidecar.
    NoSidecar,
    /// `meet-stt --probe` itself failed.
    ProbeFailed(&'a Error),
    /// The sidecar's macOS 26 guard said no, in its own words.
    TooOld(&'a str),
    /// The Mac is on 26+, but `SpeechTranscriber.isAvailable` said no.
    CannotRun,
    /// Available, but the on-device model for the locale is not installed.
    ModelMissing,
}

impl<'a> AppleUnavailable<'a> {
    fn of(apple: &'a Option<Result<Probe, Error>>) -> Self {
        match apple {
            None => Self::NoSidecar,
            Some(Err(error)) => Self::ProbeFailed(error),
            // `main.swift` only ever sets `reason` from its top-level
            // `#available(macOS 26, *)` guard — exactly the "this Mac is too
            // old" case. When `runProbe` itself reports `available: false`
            // (this Mac *is* on 26+, but `SpeechTranscriber.isAvailable` said
            // no — unsupported hardware or language), it sends no reason,
            // because there is no OS-version story to tell.
            Some(Ok(probe)) if !probe.available => match probe.reason.as_deref() {
                Some(reason) => Self::TooOld(reason),
                None => Self::CannotRun,
            },
            Some(Ok(_)) => Self::ModelMissing,
        }
    }
}

/// Say precisely which of the four ways Apple's engine can be unavailable
/// happened, so the UI can tell a genuine incompatibility (this Mac is too
/// old, or this build was not given the sidecar) apart from "not ready yet"
/// (the on-device model for this locale has not finished installing).
fn apple_unavailable_detail(apple: &Option<Result<Probe, Error>>, locale: &str) -> String {
    match AppleUnavailable::of(apple) {
        AppleUnavailable::NoSidecar => {
            "the meet-stt sidecar was not found in the app bundle".to_string()
        }
        AppleUnavailable::ProbeFailed(error) => error.to_string(),
        AppleUnavailable::TooOld(reason) => reason.to_string(),
        // Falling back to "below macOS 26" here would be a false claim, so
        // the text stays agnostic about whether it is hardware or language.
        AppleUnavailable::CannotRun => "Apple's on-device speech engine reports it cannot run \
             on this Mac (usually unsupported hardware or language, not something a download \
             fixes)"
            .to_string(),
        AppleUnavailable::ModelMissing => {
            format!("the on-device model for {locale} is not installed yet")
        }
    }
}

/// Decide which engine to use, without constructing it.
///
/// Split out from [`select`] so the UI can show "will use X" on the settings
/// screen without paying to load a 574 MB model.
pub fn resolve(preference: Preference, environment: &Environment) -> Result<Selection, Error> {
    decide(preference, environment, &probe_apple(environment))
}

/// Run `meet-stt --probe` once, if there is a sidecar to run.
fn probe_apple(environment: &Environment) -> Option<Result<Probe, Error>> {
    // Probing runs the sidecar, so do it once and reuse the answer.
    environment.sidecar.as_ref().map(|binary| {
        let probe = AppleEngine::probe(binary, &environment.locale);
        if let Err(error) = &probe {
            tracing::warn!(%error, "meet-stt --probe failed; treating Apple's engine as unavailable");
        }
        probe
    })
}

/// [`resolve`] with the probe already run, so [`options`] can ask about every
/// preference without probing once per question.
fn decide(
    preference: Preference,
    environment: &Environment,
    apple: &Option<Result<Probe, Error>>,
) -> Result<Selection, Error> {
    let apple_usable = matches!(apple, Some(Ok(probe)) if probe.is_usable_offline());

    match preference {
        Preference::AppleSpeech => {
            if apple_usable {
                return Ok(Selection {
                    engine: Kind::AppleSpeech,
                    reason: "config asked for Apple's on-device speech engine".into(),
                });
            }
            // A4's rule: a value that cannot work here is an error that says
            // why, not a silent switch. Off macOS there is no Apple engine at
            // all, so say that instead of "the sidecar was not found".
            if let Some(why) = crate::platform::APPLE_SPEECH_UNSUPPORTED {
                return Err(Error::EngineUnavailable(format!(
                    "Apple's speech engine cannot be used: {why}"
                )));
            }
            let detail = apple_unavailable_detail(apple, &environment.locale);
            Err(Error::EngineUnavailable(format!(
                "Apple's speech engine cannot be used: {detail}"
            )))
        }

        Preference::Whisper => {
            if environment.whisper_model.is_some() {
                Ok(Selection {
                    engine: Kind::Whisper,
                    reason: "config asked for the whisper engine".into(),
                })
            } else {
                Err(Error::EngineUnavailable(format!(
                    "config asked for the whisper engine but {}",
                    environment.missing_whisper_model()
                )))
            }
        }

        Preference::Auto => {
            if apple_usable {
                Ok(Selection {
                    engine: Kind::AppleSpeech,
                    reason: "this Mac has Apple's on-device speech engine, which needs no download"
                        .into(),
                })
            } else if environment.whisper_model.is_some() {
                let detail = apple_unavailable_detail(apple, &environment.locale);
                Ok(Selection {
                    engine: Kind::Whisper,
                    reason: format!(
                        "Apple's speech engine is unavailable ({detail}), so the whisper fallback \
                         is used"
                    ),
                })
            } else {
                // Two different situations look the same from here — a Mac
                // that can never run Apple's engine, and one that just hasn't
                // downloaded a whisper model yet — so name which one this is
                // rather than collapsing both into "Apple's is unavailable".
                let detail = apple_unavailable_detail(apple, &environment.locale);
                let missing = environment.missing_whisper_model();
                Err(Error::EngineUnavailable(format!(
                    "no speech engine is ready: Apple's speech engine cannot be used ({detail}), \
                     and Whisper {missing}. Download it below to continue"
                )))
            }
        }
    }
}

/// Resolve and build the engine.
pub fn select(
    preference: Preference,
    environment: &Environment,
) -> Result<(Selection, Box<dyn SttEngine>), Error> {
    let selection = resolve(preference, environment)?;

    let engine: Box<dyn SttEngine> = match selection.engine {
        Kind::AppleSpeech => {
            let binary = environment
                .sidecar
                .clone()
                .ok_or_else(|| Error::EngineUnavailable("meet-stt was not found".into()))?;
            Box::new(AppleEngine::new(binary, environment.locale.clone()))
        }
        Kind::Whisper => {
            let model = environment
                .whisper_model
                .clone()
                .ok_or_else(|| Error::EngineUnavailable("no whisper model is downloaded".into()))?;

            crate::platform::load_whisper(&model)?
        }
    };

    Ok((selection, engine))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> Environment {
        Environment {
            sidecar: None,
            locale: "en-US".into(),
            whisper_model: None,
            whisper_model_id: "large-v3-turbo-q5_0".into(),
            installed_whisper_models: Vec::new(),
        }
    }

    #[test]
    fn auto_falls_back_to_whisper_when_apple_is_missing() {
        let mut env = environment();
        env.whisper_model = Some(PathBuf::from("/tmp/ggml-small.en-q5_1.bin"));

        let selection = resolve(Preference::Auto, &env).unwrap();
        assert_eq!(selection.engine, Kind::Whisper);
        // The success case still gets to say *why* Apple's was skipped —
        // not just that whisper was picked instead.
        assert!(
            selection.reason.contains("meet-stt sidecar was not found"),
            "unhelpful reason: {}",
            selection.reason
        );
    }

    #[test]
    fn auto_with_nothing_available_is_a_clear_error() {
        let error = resolve(Preference::Auto, &environment()).unwrap_err();
        assert!(
            matches!(error, Error::EngineUnavailable(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn auto_with_nothing_available_names_why_apple_specifically_is_unavailable() {
        // TUR-81: "Apple's is unavailable" alone does not tell a user whether
        // this Mac can never run it (incompatible) or just has not finished
        // setup (not ready yet). The message must say which.
        let error = resolve(Preference::Auto, &environment()).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("meet-stt sidecar was not found"),
            "message does not explain why Apple's engine is unavailable: {message}"
        );
        assert!(
            message.contains("model large-v3-turbo-q5_0 is not downloaded (none installed)"),
            "message does not mention the actionable next step: {message}"
        );
    }

    #[test]
    fn auto_with_a_different_model_installed_names_the_missing_one() {
        let mut env = environment();
        env.installed_whisper_models = vec!["small.en-q5_1".into()];
        let message = resolve(Preference::Auto, &env).unwrap_err().to_string();
        assert!(
            message
                .contains("model large-v3-turbo-q5_0 is not downloaded (installed: small.en-q5_1)"),
            "got: {message}"
        );
    }

    #[test]
    fn forcing_apple_without_a_sidecar_names_the_sidecar() {
        let error = resolve(Preference::AppleSpeech, &environment()).unwrap_err();
        let message = error.to_string();
        // Off macOS the honest answer is "only on macOS", not a sidecar hunt.
        match crate::platform::APPLE_SPEECH_UNSUPPORTED {
            Some(why) => assert!(message.contains(why), "unhelpful message: {message}"),
            None => assert!(message.contains("meet-stt"), "unhelpful message: {message}"),
        }
    }

    #[test]
    fn forcing_apple_where_it_cannot_exist_ignores_a_sidecar_on_disk() {
        // Even with a `meet-stt` path that does not run, an OS without
        // Apple's engine names the OS, not the sidecar.
        let Some(why) = crate::platform::APPLE_SPEECH_UNSUPPORTED else {
            return;
        };
        let mut env = environment();
        env.sidecar = Some(PathBuf::from("/nowhere/meet-stt"));
        let error = resolve(Preference::AppleSpeech, &env).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Apple's speech engine cannot be used: {why}")
        );
    }

    #[test]
    fn forcing_whisper_without_a_model_does_not_silently_use_apple() {
        let error = resolve(Preference::Whisper, &environment()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "config asked for the whisper engine but model large-v3-turbo-q5_0 is not \
             downloaded (none installed)"
        );
    }

    #[test]
    fn forcing_whisper_names_the_missing_model_and_the_installed_ones() {
        // TUR-23: with only small.en installed, "no model is downloaded" was
        // false. Name the model config asked for and what is on disk instead.
        let mut env = environment();
        env.installed_whisper_models = vec!["small.en-q5_1".into(), "medium-q5_0".into()];
        let message = resolve(Preference::Whisper, &env).unwrap_err().to_string();
        assert!(
            message.contains(
                "model large-v3-turbo-q5_0 is not downloaded (installed: small.en-q5_1, medium-q5_0)"
            ),
            "got: {message}"
        );
        assert!(!message.contains("no model"), "got: {message}");
    }

    #[test]
    fn discovery_lists_every_installed_model_even_when_the_wanted_one_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let dir = crate::model::model_dir(root.path());
        std::fs::create_dir_all(&dir).unwrap();
        let small = crate::model::find("small.en-q5_1").unwrap();
        std::fs::write(dir.join(small.filename), b"not really a model").unwrap();

        let found = Environment::discover_in(Some(&dir), "en-US", "large-v3-turbo-q5_0");
        assert_eq!(found.whisper_model, None);
        assert_eq!(found.whisper_model_id, "large-v3-turbo-q5_0");
        assert_eq!(found.installed_whisper_models, vec!["small.en-q5_1"]);

        let message = resolve(Preference::Whisper, &found)
            .unwrap_err()
            .to_string();
        assert!(
            message
                .contains("model large-v3-turbo-q5_0 is not downloaded (installed: small.en-q5_1)"),
            "got: {message}"
        );

        let nowhere = Environment::discover_in(None, "en-US", "large-v3-turbo-q5_0");
        assert!(nowhere.installed_whisper_models.is_empty());
    }

    #[test]
    fn the_preference_is_plain_config_data() {
        // The exit gate says switching engines is a config change only, so the
        // preference has to round-trip through config.jsonc unaided.
        let parsed: Preference = serde_json::from_str("\"whisper\"").unwrap();
        assert_eq!(parsed, Preference::Whisper);
        let parsed: Preference = serde_json::from_str("\"apple-speech\"").unwrap();
        assert_eq!(parsed, Preference::AppleSpeech);
        assert_eq!(Preference::default(), Preference::Auto);
    }

    #[test]
    fn discovery_finds_the_whisper_model_in_the_directory_it_is_given() {
        // The app passes `<its meetings root>/.app/models`. A model that moved
        // there with the meetings folder must be found, whatever `~` says.
        let root = tempfile::tempdir().unwrap();
        let dir = crate::model::model_dir(root.path());
        std::fs::create_dir_all(&dir).unwrap();
        let spec = crate::model::find("small.en-q5_1").unwrap();
        std::fs::write(dir.join(spec.filename), b"not really a model").unwrap();

        let found = Environment::discover_in(Some(&dir), "en-US", spec.id);
        assert_eq!(found.whisper_model, Some(dir.join(spec.filename)));

        let nowhere = Environment::discover_in(None, "en-US", spec.id);
        assert_eq!(nowhere.whisper_model, None);
    }
}
