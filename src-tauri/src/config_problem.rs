//! "This setting in config.jsonc was not valid" (TUR-155): what a Settings
//! card asks for beside its values.
//!
//! The section readers skip a bad value and show its default
//! (`config::keyed`); this command tells the card which value and why, as an
//! `app/invalid-config` error the card shows with its usual error piece. The
//! card still works: a save writes only the keys the user changed.

use serde::{Deserialize, Serialize};

use crate::config;
use crate::error::{UiError, on_blocking_pool};

/// The `config.jsonc` sections a Settings card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigSection {
    /// Settings, General: the Dock and menu-bar switches.
    App,
    /// Settings, Appearance.
    Appearance,
    /// Settings, Notifications.
    Detection,
}

/// The kind the window's copy is keyed by (`src/ipc/errors.ts`).
pub const INVALID_CONFIG: &str = "invalid-config";

/// What was not valid in `section`, as the card shows it, or `None`.
pub(crate) fn problem(section: ConfigSection) -> Option<String> {
    match section {
        ConfigSection::App => config::app_checked().problem,
        ConfigSection::Appearance => config::appearance_checked().problem,
        ConfigSection::Detection => config::detection_checked().problem,
    }
}

/// `problem`, as the error the card shows.
fn as_error(problem: Option<String>) -> Option<UiError> {
    problem.map(|problem| UiError::app(INVALID_CONFIG, problem))
}

/// What in `section` of `config.jsonc` was not valid and is shown as its
/// default, or `None` when all of it was valid.
#[tauri::command]
#[specta::specta]
pub async fn config_problem(section: ConfigSection) -> Result<Option<UiError>, UiError> {
    on_blocking_pool(move || as_error(problem(section))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_problem_is_an_invalid_config_error_with_its_words() {
        let error = as_error(Some("appearance.theme 3 is not valid".into())).unwrap();
        assert_eq!((error.domain.as_str(), error.kind), ("app", INVALID_CONFIG));
        assert_eq!(error.message, "appearance.theme 3 is not valid");
        assert!(as_error(None).is_none());
    }

    #[test]
    fn the_wire_names_are_kebab_case() {
        assert_eq!(
            serde_json::to_value(ConfigSection::Detection).unwrap(),
            "detection"
        );
    }
}
