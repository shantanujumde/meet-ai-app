//! The `app` section of `config.jsonc` (TUR-76): how meet-ai behaves as an
//! app, apart from any one meeting.
//!
//! `show_in_dock_when_closed`: closing the main window hides it and leaves
//! meet-ai running in the menu bar; on macOS the Dock icon goes with the
//! window unless this is `true`. `menu_bar_countdown` (TUR-77): the next
//! meeting's countdown next to the menu-bar icon.
//!
//! Like every Settings section (TUR-155, `keyed.rs`), a bad value costs
//! only its own key: [`app`] logs it and reads that key as its default, and
//! [`app_checked`] also returns the problem for the Settings screen.
//! [`set_app`] writes only the keys its edit changed, through the
//! comment-keeping writer in `file.rs`.

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::keyed::{Checked, Keys};

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

/// `app` from the text of `config.jsonc`, key by key, with what was not
/// valid. Empty text, or no `app` key, is all defaults.
pub fn read_app(raw: &str) -> Checked<AppConfig> {
    let mut keys = Keys::read(raw, "app");
    let defaults = AppConfig::default();
    let app = AppConfig {
        show_in_dock_when_closed: keys
            .get("show_in_dock_when_closed")
            .unwrap_or(defaults.show_in_dock_when_closed),
        menu_bar_countdown: keys
            .get("menu_bar_countdown")
            .unwrap_or(defaults.menu_bar_countdown),
    };
    keys.checked(app)
}

/// `app` from the text of `config.jsonc`, each bad key logged and read as
/// its default.
pub fn parse_app(raw: &str) -> AppConfig {
    read_app(raw).value
}

/// `app` from `~/Meetings/.app/config.jsonc`, each bad key read as its
/// default (logged). Read on every window close, so an edit by hand needs no
/// restart.
pub fn app() -> AppConfig {
    parse_app(&super::raw_or_empty())
}

/// [`app`], with what was not valid, for the Settings screen.
pub fn app_checked() -> Checked<AppConfig> {
    read_app(&super::raw_or_empty())
}

/// `raw` with its `app` section set to `app`, comments and other keys kept.
#[cfg(test)]
pub fn with_app(raw: &str, app: &AppConfig) -> Result<String, ConfigError> {
    with_section(raw, "app", fields(app, None))
}

/// The keys of `app` to write: all of them, or only those that differ from
/// `before`.
fn fields(
    app: &AppConfig,
    before: Option<&AppConfig>,
) -> Vec<(&'static str, jsonc_parser::cst::CstInputValue)> {
    let mut fields = Vec::new();
    let changed = |pick: fn(&AppConfig) -> bool| before.is_none_or(|old| pick(old) != pick(app));
    if changed(|app| app.show_in_dock_when_closed) {
        fields.push((
            "show_in_dock_when_closed",
            app.show_in_dock_when_closed.into(),
        ));
    }
    if changed(|app| app.menu_bar_countdown) {
        fields.push(("menu_bar_countdown", app.menu_bar_countdown.into()));
    }
    fields
}

/// Change the `app` section of `~/Meetings/.app/config.jsonc` with `edit`,
/// keeping everything else, and return it as read back from disk.
pub fn set_app(edit: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    set_app_in(&dir, edit)
}

/// [`set_app`] for the config folder `dir`. The section `edit` starts from is
/// read from the file under the write lock, and only the keys `edit` changed
/// are written: a bad key it left alone stays as the user wrote it.
fn set_app_in(
    dir: &std::path::Path,
    edit: impl FnOnce(&mut AppConfig),
) -> Result<AppConfig, ConfigError> {
    write_in(dir, |raw| {
        let before = parse_app(raw);
        let mut app = before;
        edit(&mut app);
        with_section(raw, "app", fields(&app, Some(&before)))
    })?;
    Ok(parse_app(&read_in(dir)?))
}

#[cfg(test)]
mod tests {
    use super::super::file::SCHEMA;
    use super::*;

    #[test]
    fn no_section_is_the_dock_icon_going_with_the_window() {
        for raw in ["", "{}", r#"{ "app": {} }"#, r#"{ "detection": {} }"#] {
            assert_eq!(parse_app(raw), AppConfig::default(), "{raw:?}");
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
        );
        assert!(app.show_in_dock_when_closed);
    }

    #[test]
    fn a_bad_value_is_a_problem_and_reads_as_its_default_keeping_the_rest() {
        for raw in [
            r#"{ "app": { "show_in_dock_when_closed": "yes" } }"#,
            r#"{ "app": null }"#,
            "{ not json",
        ] {
            let read = read_app(raw);
            assert!(read.problem.is_some(), "{raw:?}");
            assert_eq!(read.value, AppConfig::default(), "{raw:?}");
        }
        let read = read_app(
            r#"{ "app": { "show_in_dock_when_closed": "yes", "menu_bar_countdown": true } }"#,
        );
        assert!(read.value.menu_bar_countdown, "the good key is kept");
        assert!(read.problem.is_some());
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
        assert_eq!(parse_app(&written), on);

        // And back off, in place rather than as a second key.
        let off = with_app(&written, &AppConfig::default()).unwrap();
        assert_eq!(parse_app(&off), AppConfig::default());
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
        assert_eq!(parse_app(&read_in(&dir).unwrap()), on);
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
        let app = parse_app(r#"{ "app": { "menu_bar_countdown": true } }"#);
        assert!(app.menu_bar_countdown);
        assert!(
            !app.show_in_dock_when_closed,
            "the other key keeps its default"
        );
        assert!(
            read_app(r#"{ "app": { "menu_bar_countdown": 1 } }"#)
                .problem
                .is_some()
        );
    }

    #[test]
    fn saving_the_countdown_keeps_the_dock_switch() {
        let both = AppConfig {
            show_in_dock_when_closed: true,
            menu_bar_countdown: true,
        };
        let written = with_app("", &both).unwrap();
        assert_eq!(parse_app(&written), both);
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
    fn saving_beside_a_bad_key_writes_the_change_and_leaves_the_bad_key() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = r#"{ "app": { "show_in_dock_when_closed": "yes" } }"#;
        std::fs::write(dir.join(super::super::FILE), bad).unwrap();

        let saved = set_app_in(&dir, |app| app.menu_bar_countdown = true).unwrap();
        assert!(saved.menu_bar_countdown);
        let written = read_in(&dir).unwrap();
        assert!(
            written.contains(r#""show_in_dock_when_closed": "yes""#),
            "{written}"
        );
    }

    #[test]
    fn saving_into_a_file_that_does_not_parse_is_refused_and_the_file_kept() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = "{ not json";
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
            let saved = parse_app(&read_in(&dir).unwrap());
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
