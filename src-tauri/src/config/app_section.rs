//! The `app` section of `config.jsonc` (TUR-76): how meet-ai behaves as an
//! app, apart from any one meeting.
//!
//! One key so far, `show_in_dock_when_closed`. Closing the main window hides
//! it and leaves meet-ai running in the menu bar; on macOS the Dock icon goes
//! with the window unless this is `true`.
//!
//! Like `detection`, the app-facing reader [`app`] logs a bad section and
//! runs with the defaults, so a typo never stops the app from starting.
//! [`parse_app`] returns the error for a caller that wants to show it, and
//! [`set_app`] writes the section back through the comment-keeping writer in
//! `file.rs`.

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::read_section;

/// `app` in `config.jsonc`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Keep the Dock icon while the main window is closed (macOS). Off by
    /// default: a closed window leaves only the menu-bar item, like Granola.
    pub show_in_dock_when_closed: bool,
}

/// `app` as written. Every key optional, so a missing one is the default.
#[derive(Debug, Default, Deserialize)]
struct RawApp {
    show_in_dock_when_closed: Option<bool>,
}

/// `app` from the text of `config.jsonc`. Empty text, or no `app` key, is
/// all defaults.
pub fn parse_app(raw: &str) -> Result<AppConfig, ConfigError> {
    let app: RawApp = read_section(raw, "app")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    let defaults = AppConfig::default();
    Ok(AppConfig {
        show_in_dock_when_closed: app
            .show_in_dock_when_closed
            .unwrap_or(defaults.show_in_dock_when_closed),
    })
}

/// [`parse_app`], with a bad section logged and replaced by the defaults.
fn app_or_defaults(raw: &str) -> AppConfig {
    parse_app(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's app section is not valid; using defaults");
        AppConfig::default()
    })
}

/// `app` from `~/Meetings/.app/config.jsonc`, or the defaults if the file or
/// section is missing or not valid (logged). Read on every window close, so
/// an edit by hand needs no restart.
pub fn app() -> AppConfig {
    app_or_defaults(&super::raw_or_empty())
}

/// `raw` with its `app` section set to `app`, comments and other keys kept.
pub fn with_app(raw: &str, app: &AppConfig) -> Result<String, ConfigError> {
    with_section(
        raw,
        "app",
        vec![(
            "show_in_dock_when_closed",
            app.show_in_dock_when_closed.into(),
        )],
    )
}

/// Save `app` into `~/Meetings/.app/config.jsonc`, keeping everything else,
/// and return it as read back from disk.
pub fn set_app(app: &AppConfig) -> Result<AppConfig, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    write_in(&dir, |raw| with_app(raw, app))?;
    parse_app(&read_in(&dir)?)
}

#[cfg(test)]
mod tests {
    use super::super::file::SCHEMA;
    use super::*;

    #[test]
    fn no_section_is_the_dock_icon_going_with_the_window() {
        for raw in ["", "{}", r#"{ "app": {} }"#, r#"{ "detection": {} }"#] {
            assert_eq!(parse_app(raw).unwrap(), AppConfig::default(), "{raw:?}");
        }
        assert!(!AppConfig::default().show_in_dock_when_closed);
    }

    #[test]
    fn the_switch_turns_on_with_a_config_edit_alone() {
        let app = parse_app(
            r#"{
                // JSONC: comments allowed
                "app": { "show_in_dock_when_closed": true }
            }"#,
        )
        .unwrap();
        assert!(app.show_in_dock_when_closed);
    }

    #[test]
    fn a_bad_section_is_an_error_and_the_app_reader_uses_defaults() {
        for raw in [
            r#"{ "app": { "show_in_dock_when_closed": "yes" } }"#,
            r#"{ "app": null }"#,
            "{ not json",
        ] {
            assert!(parse_app(raw).is_err(), "{raw:?}");
            assert_eq!(app_or_defaults(raw), AppConfig::default(), "{raw:?}");
        }
    }

    #[test]
    fn saving_keeps_comments_and_other_sections() {
        let raw = r#"{
  // mine
  "detection": { "processes": false }
}"#;
        let on = AppConfig {
            show_in_dock_when_closed: true,
        };
        let written = with_app(raw, &on).unwrap();
        assert!(written.contains("// mine"), "{written}");
        assert!(written.contains(r#""processes": false"#), "{written}");
        assert_eq!(parse_app(&written).unwrap(), on);

        // And back off, in place rather than as a second key.
        let off = with_app(&written, &AppConfig::default()).unwrap();
        assert_eq!(parse_app(&off).unwrap(), AppConfig::default());
        assert_eq!(off.matches("show_in_dock_when_closed").count(), 1, "{off}");
    }

    #[test]
    fn saving_to_disk_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        let on = AppConfig {
            show_in_dock_when_closed: true,
        };
        write_in(&dir, |raw| with_app(raw, &on)).unwrap();
        assert_eq!(parse_app(&read_in(&dir).unwrap()).unwrap(), on);
    }

    #[test]
    fn the_schema_default_matches_the_code_default() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let key = &schema["properties"]["app"]["properties"]["show_in_dock_when_closed"];
        assert_eq!(key["type"], "boolean");
        assert_eq!(
            key["default"],
            AppConfig::default().show_in_dock_when_closed
        );
    }
}
