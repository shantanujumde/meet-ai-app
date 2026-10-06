//! The Parakeet choice (TUR-62): only when config asks for it, and only when
//! both its model and ONNX Runtime are here.
//!
//! ONNX Runtime is linked into the binary on macOS and Linux. On Windows it is
//! a separate `onnxruntime.dll` that the installer does not ship yet, so the
//! picker says so up front instead of offering a 670 MB download that could
//! not run. Both rules take the runtime's state as an argument, so every OS
//! tests both answers.

use super::{Availability, Environment, Kind, Selection, options::PARAKEET_NEEDS_A_MODEL};
use crate::Error;
use crate::model::parakeet::PARAKEET_V3;

/// Why Parakeet cannot be picked in this build: no ONNX Runtime beside it.
pub const PARAKEET_NOT_IN_THIS_BUILD: &str = "Not ready on this system yet: this copy of meet-ai does not include the ONNX Runtime \
     library Parakeet runs on.";

/// Why ONNX Runtime cannot be used, in a sentence for a log or an error, or
/// `None` when it is here. A file check, never a load.
pub fn parakeet_runtime_missing() -> Option<String> {
    crate::platform::onnx_runtime_missing()
}

/// `Preference::Parakeet`, given whether ONNX Runtime is missing and why.
pub(super) fn decide(
    environment: &Environment,
    runtime_missing: Option<&str>,
) -> Result<Selection, Error> {
    // The runtime first: with no runtime, a download would not help.
    if let Some(why) = runtime_missing {
        return Err(Error::EngineUnavailable(format!(
            "config asked for the Parakeet engine but {why}"
        )));
    }
    match environment.parakeet_model {
        Some(_) => Ok(Selection {
            engine: Kind::Parakeet,
            reason: "config asked for the Parakeet engine".into(),
        }),
        None => Err(Error::EngineUnavailable(format!(
            "config asked for the Parakeet engine but its model ({}) is not downloaded",
            PARAKEET_V3.id
        ))),
    }
}

/// The picker's answer for Parakeet.
pub(super) fn availability(environment: &Environment, runtime_missing: bool) -> Availability {
    let reason = if runtime_missing {
        Some(PARAKEET_NOT_IN_THIS_BUILD)
    } else if environment.parakeet_model.is_none() {
        Some(PARAKEET_NEEDS_A_MODEL)
    } else {
        None
    };
    Availability {
        available: reason.is_none(),
        reason: reason.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{Preference, resolve};
    use super::*;

    fn environment(model: bool) -> Environment {
        Environment {
            sidecar: None,
            locale: "en-US".into(),
            whisper_model: None,
            whisper_model_id: "large-v3-turbo-q5_0".into(),
            installed_whisper_models: Vec::new(),
            parakeet_model: model.then(|| PathBuf::from("/tmp/parakeet-tdt-0.6b-v3-int8")),
            spoken: Default::default(),
        }
    }

    const NO_DLL: &str = "the Parakeet engine needs ONNX Runtime (onnxruntime.dll), which is \
                          not installed at C:\\meet-ai\\onnxruntime.dll";

    #[test]
    fn parakeet_is_used_only_with_its_model() {
        let message = decide(&environment(false), None).unwrap_err().to_string();
        assert_eq!(
            message,
            "config asked for the Parakeet engine but its model (parakeet-tdt-0.6b-v3) is not \
             downloaded"
        );
        let selection = decide(&environment(true), None).unwrap();
        assert_eq!(selection.engine, Kind::Parakeet);
        assert_eq!(selection.engine.name(), super::super::PARAKEET);
    }

    #[test]
    fn without_onnx_runtime_the_error_says_so_even_with_the_model() {
        for model in [false, true] {
            let message = decide(&environment(model), Some(NO_DLL))
                .unwrap_err()
                .to_string();
            assert_eq!(
                message,
                format!("config asked for the Parakeet engine but {NO_DLL}")
            );
        }
    }

    fn no(reason: &str) -> Availability {
        Availability {
            available: false,
            reason: Some(reason.to_string()),
        }
    }

    #[test]
    fn the_picker_says_not_ready_before_it_asks_for_a_download() {
        assert_eq!(
            availability(&environment(false), true),
            no(PARAKEET_NOT_IN_THIS_BUILD)
        );
        assert_eq!(
            availability(&environment(true), true),
            no(PARAKEET_NOT_IN_THIS_BUILD)
        );
        assert_eq!(
            availability(&environment(false), false),
            no(PARAKEET_NEEDS_A_MODEL)
        );
        assert_eq!(
            availability(&environment(true), false),
            Availability {
                available: true,
                reason: None
            }
        );
    }

    #[test]
    fn resolve_and_options_agree_with_this_build() {
        // Whatever this OS's answer is, the registry and the picker give the
        // same one, and Auto never lands on Parakeet.
        let env = environment(true);
        let runtime_missing = parakeet_runtime_missing();
        assert_eq!(
            resolve(Preference::Parakeet, &env).is_ok(),
            runtime_missing.is_none()
        );
        let found = super::super::options(&env);
        assert_eq!(found.parakeet.available, runtime_missing.is_none());
        assert_ne!(found.auto, Some(Kind::Parakeet));
        assert!(resolve(Preference::Auto, &env).is_err());
    }
}
