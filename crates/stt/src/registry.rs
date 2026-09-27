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

use std::path::PathBuf;

use crate::apple::AppleEngine;
use crate::{Error, SttEngine};

// whisper-rs compiles whisper.cpp for the target, so the engine itself is
// macOS-only for now (SPEC §8.2 turns on `vulkan`/`cuda` at Windows port
// time). Selection *logic* is not platform-specific and must keep compiling
// everywhere, so only the construction below is gated — that is what keeps
// `just check-windows` meaningful instead of excluding this crate from it.
#[cfg(target_os = "macos")]
use crate::whisper::{WhisperConfig, WhisperEngine};

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
/// every engine on every platform, while `WhisperEngine` only exists on macOS
/// today. The engines reference these back, so there is still one source of
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
}

impl Environment {
    /// Discover the environment from the filesystem.
    ///
    /// Deliberately does not download anything: this answers "what can we do
    /// right now, offline", which is the question selection needs.
    pub fn discover(locale: &str, model_id: &str) -> Self {
        let whisper_model = crate::model::default_model_dir()
            .ok()
            .and_then(|dir| crate::model::find(model_id).map(|spec| dir.join(spec.filename)))
            .filter(|path| path.is_file());

        Self {
            sidecar: AppleEngine::discover(),
            locale: locale.to_string(),
            whisper_model,
        }
    }
}

/// Decide which engine to use, without constructing it.
///
/// Split out from [`select`] so the UI can show "will use X" on the settings
/// screen without paying to load a 574 MB model.
pub fn resolve(preference: Preference, environment: &Environment) -> Result<Selection, Error> {
    // Probing runs the sidecar, so do it once and reuse the answer.
    let apple = environment.sidecar.as_ref().map(|binary| {
        let probe = AppleEngine::probe(binary, &environment.locale);
        if let Err(error) = &probe {
            tracing::warn!(%error, "meet-stt --probe failed; treating Apple's engine as unavailable");
        }
        probe
    });
    let apple_usable = matches!(&apple, Some(Ok(probe)) if probe.is_usable_offline());

    match preference {
        Preference::AppleSpeech => {
            if apple_usable {
                return Ok(Selection {
                    engine: Kind::AppleSpeech,
                    reason: "config asked for Apple's on-device speech engine".into(),
                });
            }
            // Say precisely which of the three ways it can be unavailable
            // happened, so the UI can offer the right next step.
            let detail = match &apple {
                None => "the meet-stt sidecar was not found in the app bundle".to_string(),
                Some(Err(error)) => error.to_string(),
                Some(Ok(probe)) if !probe.available => probe
                    .reason
                    .clone()
                    .unwrap_or_else(|| "this Mac is below macOS 26".into()),
                Some(Ok(_)) => format!(
                    "the on-device model for {} is not installed yet",
                    environment.locale
                ),
            };
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
                Err(Error::EngineUnavailable(
                    "config asked for the whisper engine but no model is downloaded yet".into(),
                ))
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
                Ok(Selection {
                    engine: Kind::Whisper,
                    reason: "Apple's speech engine is unavailable, so the whisper fallback is used"
                        .into(),
                })
            } else {
                Err(Error::EngineUnavailable(
                    "no speech engine is ready: Apple's is unavailable and no whisper model is \
                     downloaded"
                        .into(),
                ))
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

            #[cfg(target_os = "macos")]
            {
                Box::new(WhisperEngine::load(&model, WhisperConfig::default())?)
            }
            // The Windows port (SPEC §8.2) turns whisper-rs back on with the
            // `vulkan`/`cuda` features and deletes this arm. Until then the
            // seam guard needs the crate to compile for Windows, and it can
            // only do that if there is no whisper engine to construct.
            #[cfg(not(target_os = "macos"))]
            {
                let _ = model;
                return Err(Error::EngineUnavailable(
                    "the whisper engine is not built for this platform yet".into(),
                ));
            }
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
        }
    }

    #[test]
    fn auto_falls_back_to_whisper_when_apple_is_missing() {
        let mut env = environment();
        env.whisper_model = Some(PathBuf::from("/tmp/ggml-small.en-q5_1.bin"));

        let selection = resolve(Preference::Auto, &env).unwrap();
        assert_eq!(selection.engine, Kind::Whisper);
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
    fn forcing_apple_without_a_sidecar_names_the_sidecar() {
        let error = resolve(Preference::AppleSpeech, &environment()).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("meet-stt"), "unhelpful message: {message}");
    }

    #[test]
    fn forcing_whisper_without_a_model_does_not_silently_use_apple() {
        let error = resolve(Preference::Whisper, &environment()).unwrap_err();
        assert!(error.to_string().contains("no model is downloaded"));
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
}
