//! The `appearance` section of `config.jsonc` (TUR-102): how the window looks.
//!
//! `theme` is `"system"`, `"light"` or `"dark"`: the colour scheme the window
//! uses, where `"system"` follows the OS. `glass` turns the see-through look
//! of the sidebar and the record prompt on or off; off, both are solid.
//!
//! Like `app`, the reader [`appearance`] logs a bad section and uses the
//! defaults, so a typo only costs the look, never a start. [`set_appearance`]
//! refuses to save over a section that does not parse, so the user's file is
//! never replaced by defaults.

use serde::{Deserialize, Serialize};

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::read_section;

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

/// `appearance` as written. Every key optional, so a missing one is the default.
#[derive(Debug, Default, Deserialize)]
struct RawAppearance {
    theme: Option<Theme>,
    glass: Option<bool>,
}

/// `appearance` from the text of `config.jsonc`. Empty text, or no
/// `appearance` key, is all defaults.
pub fn parse_appearance(raw: &str) -> Result<AppearanceConfig, ConfigError> {
    let section: RawAppearance = read_section(raw, SECTION)
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    let defaults = AppearanceConfig::default();
    Ok(AppearanceConfig {
        theme: section.theme.unwrap_or(defaults.theme),
        glass: section.glass.unwrap_or(defaults.glass),
    })
}

/// [`parse_appearance`], with a bad section logged and replaced by the defaults.
fn appearance_or_defaults(raw: &str) -> AppearanceConfig {
    parse_appearance(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's appearance section is not valid; using defaults");
        AppearanceConfig::default()
    })
}

/// `appearance` from `~/Meetings/.app/config.jsonc`, or the defaults if the
/// file or section is missing or not valid (logged).
pub fn appearance() -> AppearanceConfig {
    appearance_or_defaults(&super::raw_or_empty())
}

/// `raw` with its `appearance` section set, comments and other keys kept.
pub fn with_appearance(raw: &str, appearance: &AppearanceConfig) -> Result<String, ConfigError> {
    with_section(
        raw,
        SECTION,
        vec![
            ("theme", appearance.theme.as_str().into()),
            ("glass", appearance.glass.into()),
        ],
    )
}

/// Save `appearance` to `~/Meetings/.app/config.jsonc` and return it as read
/// back from disk.
pub fn set_appearance(appearance: AppearanceConfig) -> Result<AppearanceConfig, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    set_appearance_in(&dir, appearance)
}

/// [`set_appearance`] for the config folder `dir`. A section that does not
/// parse is refused, never replaced, and the message reaches the Settings row.
fn set_appearance_in(
    dir: &std::path::Path,
    appearance: AppearanceConfig,
) -> Result<AppearanceConfig, ConfigError> {
    write_in(dir, |raw| {
        parse_appearance(raw)?;
        with_appearance(raw, &appearance)
    })?;
    parse_appearance(&read_in(dir)?)
}

#[cfg(test)]
mod tests {
    use super::super::file::SCHEMA;
    use super::*;

    #[test]
    fn no_section_is_the_system_theme_with_glass_on() {
        for raw in ["", "{}", r#"{ "appearance": {} }"#, r#"{ "app": {} }"#] {
            assert_eq!(
                parse_appearance(raw).unwrap(),
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
            let read = parse_appearance(&raw).unwrap();
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
        )
        .unwrap();
        assert!(!read.glass);
        assert_eq!(read.theme, Theme::System);
    }

    #[test]
    fn a_bad_section_is_an_error_and_the_reader_uses_defaults() {
        for raw in [
            r#"{ "appearance": { "theme": "sepia" } }"#,
            r#"{ "appearance": { "glass": "yes" } }"#,
            r#"{ "appearance": null }"#,
            "{ not json",
        ] {
            assert!(parse_appearance(raw).is_err(), "{raw:?}");
            assert_eq!(
                appearance_or_defaults(raw),
                AppearanceConfig::default(),
                "{raw:?}"
            );
        }
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
        assert_eq!(parse_appearance(&written).unwrap(), dark);

        let back = with_appearance(&written, &AppearanceConfig::default()).unwrap();
        assert_eq!(
            parse_appearance(&back).unwrap(),
            AppearanceConfig::default()
        );
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
        assert_eq!(parse_appearance(&read_in(&dir).unwrap()).unwrap(), light);
    }

    #[test]
    fn saving_over_a_bad_section_is_refused_and_the_file_kept() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = r#"{ "appearance": { "theme": 3 } }"#;
        std::fs::write(dir.join(super::super::FILE), bad).unwrap();

        let error = set_appearance_in(&dir, AppearanceConfig::default()).unwrap_err();
        assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
        assert_eq!(read_in(&dir).unwrap(), bad);
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
