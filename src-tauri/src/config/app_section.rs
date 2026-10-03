//! The `app` section of `config.jsonc` (TUR-76): how meet-ai behaves as an
//! app, apart from any one meeting.
//!
//! `show_in_dock_when_closed`: closing the main window hides it and leaves
//! meet-ai running in the menu bar; on macOS the Dock icon goes with the
//! window unless this is `true`. `menu_bar_countdown` (TUR-77): the next
//! meeting's countdown next to the menu-bar icon.
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
    /// Show the next meeting next to the menu-bar icon ("Weekly sync in
    /// 12m") within an hour of its start (TUR-77, macOS). Off by default.
    pub menu_bar_countdown: bool,
}

/// `app` as written. Every key optional, so a missing one is the default.
#[derive(Debug, Default, Deserialize)]
struct RawApp {
    show_in_dock_when_closed: Option<bool>,
    menu_bar_countdown: Option<bool>,
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
        menu_bar_countdown: app
            .menu_bar_countdown
            .unwrap_or(defaults.menu_bar_countdown),
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
        vec![
            (
                "show_in_dock_when_closed",
                app.show_in_dock_when_closed.into(),
            ),
            ("menu_bar_countdown", app.menu_bar_countdown.into()),
        ],
    )
}

/// Change the `app` section of `~/Meetings/.app/config.jsonc` with `edit`,
/// keeping everything else, and return it as read back from disk.
pub fn set_app(edit: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    set_app_in(&dir, edit)
}

/// [`set_app`] for the config folder `dir`. The section `edit` starts from is
/// parsed from the file under the write lock, so the keys it leaves alone are
/// the ones on disk. A section that does not parse is refused, never
/// replaced by defaults, and the message reaches the Settings switch.
fn set_app_in(
    dir: &std::path::Path,
    edit: impl FnOnce(&mut AppConfig),
) -> Result<AppConfig, ConfigError> {
    write_in(dir, |raw| {
        let mut app = parse_app(raw)?;
        edit(&mut app);
        with_app(raw, &app)
    })?;
    parse_app(&read_in(dir)?)
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
            ..AppConfig::default()
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
            ..AppConfig::default()
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

    #[test]
    fn the_countdown_is_off_by_default_and_on_with_a_config_edit() {
        assert!(!AppConfig::default().menu_bar_countdown);
        let app = parse_app(r#"{ "app": { "menu_bar_countdown": true } }"#).unwrap();
        assert!(app.menu_bar_countdown);
        assert!(
            !app.show_in_dock_when_closed,
            "the other key keeps its default"
        );
        assert!(parse_app(r#"{ "app": { "menu_bar_countdown": 1 } }"#).is_err());
    }

    #[test]
    fn saving_the_countdown_keeps_the_dock_switch() {
        let both = AppConfig {
            show_in_dock_when_closed: true,
            menu_bar_countdown: true,
        };
        let written = with_app("", &both).unwrap();
        assert_eq!(parse_app(&written).unwrap(), both);
    }

    #[test]
    fn saving_one_key_keeps_the_other_as_it_is_on_disk() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        write_in(&dir, |raw| {
            with_app(
                raw,
                &AppConfig {
                    show_in_dock_when_closed: true,
                    ..AppConfig::default()
                },
            )
        })
        .unwrap();
        let saved = set_app_in(&dir, |app| app.menu_bar_countdown = true).unwrap();
        assert_eq!(
            saved,
            AppConfig {
                show_in_dock_when_closed: true,
                menu_bar_countdown: true,
            }
        );
    }

    #[test]
    fn saving_over_a_bad_section_is_refused_and_the_file_kept() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = r#"{ "app": { "show_in_dock_when_closed": "yes" } }"#;
        std::fs::write(dir.join(super::super::FILE), bad).unwrap();

        let error = set_app_in(&dir, |app| app.menu_bar_countdown = true).unwrap_err();
        assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
        assert_eq!(read_in(&dir).unwrap(), bad);
    }

    #[test]
    fn two_saves_at_once_both_land() {
        // Without the write lock both threads read the file before either
        // wrote it, and the second rename dropped the first one's key.
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        for round in 0..20 {
            let on = round % 2 == 0;
            std::thread::scope(|scope| {
                scope.spawn(|| set_app_in(&dir, |app| app.show_in_dock_when_closed = on).unwrap());
                scope.spawn(|| set_app_in(&dir, |app| app.menu_bar_countdown = on).unwrap());
            });
            let saved = parse_app(&read_in(&dir).unwrap()).unwrap();
            assert_eq!(
                saved,
                AppConfig {
                    show_in_dock_when_closed: on,
                    menu_bar_countdown: on,
                },
                "round {round}"
            );
        }
    }

    #[test]
    fn the_schema_countdown_default_matches_the_code_default() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let key = &schema["properties"]["app"]["properties"]["menu_bar_countdown"];
        assert_eq!(key["type"], "boolean");
        assert_eq!(key["default"], AppConfig::default().menu_bar_countdown);
    }
}
