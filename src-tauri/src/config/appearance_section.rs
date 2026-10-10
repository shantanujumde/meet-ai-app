//! The `appearance` section of `config.jsonc` (TUR-102): how the window looks.
//!
//! `theme` is `"system"`, `"light"` or `"dark"`: the colour scheme the window
//! uses, where `"system"` follows the OS. `glass` turns the see-through look
//! of the sidebar and the record prompt on or off; off, both are solid.
//!
//! Like every Settings section (TUR-155, `keyed.rs`), a bad value costs
//! only its own key: `"theme": 3` reads as the system theme and keeps
//! `glass`, and [`appearance_checked`] tells the Settings screen why.
//! [`set_appearance`] writes only the keys that changed, so a save never
//! fails because of the other one, and a bad key it did not touch is left
//! as the user wrote it.

use serde::{Deserialize, Serialize};

use super::error::ConfigError;
use super::keyed::{Checked, Keys};
use super::section::{Fields, Section};

/// The section's name in `config.jsonc`.
const SECTION: &str = "appearance";

/// `appearance.theme`: which colour scheme the window uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the OS, and change with it.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// The value as written in `config.jsonc`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// `appearance` in `config.jsonc`, as the Settings screen shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceConfig {
    pub theme: Theme,
    /// The see-through sidebar and record prompt. On by default.
    pub glass: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            glass: true,
        }
    }
}

impl Section for AppearanceConfig {
    const NAME: &'static str = SECTION;

    fn from_keys(keys: &mut Keys) -> Self {
        let defaults = Self::default();
        Self {
            theme: keys.get("theme").unwrap_or(defaults.theme),
            glass: keys.get("glass").unwrap_or(defaults.glass),
        }
    }

    fn fields(&self, before: Option<&Self>) -> Fields {
        let mut fields = Vec::new();
        if before.is_none_or(|old| old.theme != self.theme) {
            fields.push(("theme", self.theme.as_str().into()));
        }
        if before.is_none_or(|old| old.glass != self.glass) {
            fields.push(("glass", self.glass.into()));
        }
        fields
    }
}

/// `appearance` from `~/Meetings/.app/config.jsonc`, each bad key read as
/// its default (logged).
pub fn appearance() -> AppearanceConfig {
    AppearanceConfig::current()
}

/// [`appearance`], with what was not valid, for the Settings screen.
pub fn appearance_checked() -> Checked<AppearanceConfig> {
    AppearanceConfig::current_checked()
}

/// Save `appearance` to `~/Meetings/.app/config.jsonc` and return it as read
/// back from disk. Only the keys that differ from the file, read under the
/// write lock, are written.
pub fn set_appearance(appearance: AppearanceConfig) -> Result<AppearanceConfig, ConfigError> {
    AppearanceConfig::update(|_| Ok::<_, ConfigError>(appearance))
}

#[cfg(test)]
mod tests {
    use super::super::file::SCHEMA;
    use super::super::file::read_in;
    use super::*;

    // The names these tests were written against (TUR-176 moved the code
    // into `Section`).
    fn read_appearance(raw: &str) -> Checked<AppearanceConfig> {
        AppearanceConfig::read(raw)
    }
    fn parse_appearance(raw: &str) -> AppearanceConfig {
        AppearanceConfig::parse(raw)
    }
    fn with_appearance(raw: &str, appearance: &AppearanceConfig) -> Result<String, ConfigError> {
        AppearanceConfig::with(raw, appearance)
    }
    fn set_appearance_in(
        dir: &std::path::Path,
        appearance: AppearanceConfig,
    ) -> Result<AppearanceConfig, ConfigError> {
        AppearanceConfig::update_in(dir, |_| Ok::<_, ConfigError>(appearance))
    }

    #[test]
    fn no_section_is_the_system_theme_with_glass_on() {
        for raw in ["", "{}", r#"{ "appearance": {} }"#, r#"{ "app": {} }"#] {
            assert_eq!(
                parse_appearance(raw),
                AppearanceConfig::default(),
                "{raw:?}"
            );
        }
        assert_eq!(AppearanceConfig::default().theme, Theme::System);
        assert!(AppearanceConfig::default().glass);
    }

    #[test]
    fn each_theme_reads_from_its_word() {
        for (word, theme) in [
            ("system", Theme::System),
            ("light", Theme::Light),
            ("dark", Theme::Dark),
        ] {
            let raw = format!(r#"{{ "appearance": {{ "theme": "{word}" }} }}"#);
            let read = parse_appearance(&raw);
            assert_eq!(read.theme, theme, "{word}");
            assert!(read.glass, "the other key keeps its default");
            assert_eq!(theme.as_str(), word);
        }
    }

    #[test]
    fn glass_turns_off_with_a_config_edit_alone() {
        let read = parse_appearance(
            r#"{
                // JSONC: comments allowed
                "appearance": { "glass": false }
            }"#,
        );
        assert!(!read.glass);
        assert_eq!(read.theme, Theme::System);
    }

    #[test]
    fn a_bad_value_is_a_problem_and_reads_as_its_default() {
        for raw in [
            r#"{ "appearance": { "theme": "sepia" } }"#,
            r#"{ "appearance": { "glass": "yes" } }"#,
            r#"{ "appearance": null }"#,
            "{ not json",
        ] {
            let read = read_appearance(raw);
            assert!(read.problem.is_some(), "{raw:?}");
            assert_eq!(read.value, AppearanceConfig::default(), "{raw:?}");
        }
    }

    #[test]
    fn a_bad_theme_keeps_the_good_glass_and_says_why() {
        let read = read_appearance(r#"{ "appearance": { "theme": 3, "glass": false } }"#);
        assert_eq!(read.value.theme, Theme::System);
        assert!(!read.value.glass, "the good key is kept");
        let problem = read.problem.unwrap();
        assert!(problem.starts_with("appearance.theme 3"), "{problem}");
    }

    #[test]
    fn saving_keeps_comments_and_other_sections_and_writes_in_place() {
        let raw = r#"{
  // mine
  "app": { "menu_bar_countdown": true }
}"#;
        let dark = AppearanceConfig {
            theme: Theme::Dark,
            glass: false,
        };
        let written = with_appearance(raw, &dark).unwrap();
        assert!(written.contains("// mine"), "{written}");
        assert!(
            written.contains(r#""menu_bar_countdown": true"#),
            "{written}"
        );
        assert_eq!(parse_appearance(&written), dark);

        let back = with_appearance(&written, &AppearanceConfig::default()).unwrap();
        assert_eq!(parse_appearance(&back), AppearanceConfig::default());
        assert_eq!(back.matches(r#""theme""#).count(), 1, "{back}");
        assert_eq!(back.matches(r#""glass""#).count(), 1, "{back}");
    }

    #[test]
    fn saving_to_disk_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        let light = AppearanceConfig {
            theme: Theme::Light,
            glass: true,
        };
        assert_eq!(set_appearance_in(&dir, light).unwrap(), light);
        assert_eq!(parse_appearance(&read_in(&dir).unwrap()), light);
    }

    #[test]
    fn saving_beside_a_bad_theme_lands_and_leaves_the_theme_as_written() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = r#"{ "appearance": { "theme": 3 } }"#;
        std::fs::write(dir.join(super::super::FILE), bad).unwrap();

        // Settings shows System with glass on; the user turns glass off.
        let saved = set_appearance_in(
            &dir,
            AppearanceConfig {
                glass: false,
                ..AppearanceConfig::default()
            },
        )
        .unwrap();
        assert!(!saved.glass);
        assert_eq!(saved.theme, Theme::System);
        let written = read_in(&dir).unwrap();
        assert!(written.contains(r#""theme": 3"#), "{written}");

        // Picking a theme fixes the bad key.
        let dark = AppearanceConfig {
            theme: Theme::Dark,
            glass: false,
        };
        assert_eq!(set_appearance_in(&dir, dark).unwrap(), dark);
        assert_eq!(read_appearance(&read_in(&dir).unwrap()).problem, None);
    }

    #[test]
    fn the_schema_documents_both_keys_and_their_defaults() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let section = &schema["properties"]["appearance"]["properties"];
        assert_eq!(section["theme"]["default"], "system");
        assert_eq!(
            section["theme"]["enum"],
            serde_json::json!(["system", "light", "dark"])
        );
        assert_eq!(section["glass"]["type"], "boolean");
        assert_eq!(
            section["glass"]["default"],
            AppearanceConfig::default().glass
        );
    }

    #[test]
    fn the_ipc_shape_is_camel_case_words() {
        let json = serde_json::to_value(AppearanceConfig {
            theme: Theme::Dark,
            glass: false,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "theme": "dark", "glass": false }));
    }
}
