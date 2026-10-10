//! `detection.never_detect` (TUR-143): the apps the user said are never a
//! call, from a prompt's "Never for <App>" or Settings → Notifications'
//! "Never detect" list. Each entry is an app's name as the prompt showed it
//! ("WhatsApp") or, written by hand, its id (`net.whatsapp.WhatsApp`);
//! `detect::call_start` matches either, without case.
//!
//! Its own reader beside `detection_section.rs`, like `audio_mic.rs` beside
//! `audio`: a list is not `Copy`, and the switches are read far more often.
//! Like the switches, a bad value is logged and read as the default (no
//! apps): this only quiets prompts, so a typo must never stop the app.

use std::path::Path;

use jsonc_parser::cst::CstInputValue;
use serde::Deserialize;

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::read_section;

/// The key in the `detection` section.
const KEY: &str = "never_detect";

/// Only this key of `detection`; the switches are read elsewhere.
#[derive(Debug, Default, Deserialize)]
struct RawNever {
    never_detect: Option<Vec<String>>,
}

/// `apps` trimmed, without empty entries and without repeats (ignoring
/// case; the first spelling wins), in their order.
pub fn tidy(apps: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for app in apps {
        let app = app.trim();
        if !app.is_empty() && !out.iter().any(|kept| kept.eq_ignore_ascii_case(app)) {
            out.push(app.to_string());
        }
    }
    out
}

/// The list from the text of `config.jsonc`.
pub fn parse(raw: &str) -> Result<Vec<String>, ConfigError> {
    let section: RawNever = read_section(raw, "detection")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    Ok(tidy(section.never_detect.unwrap_or_default()))
}

fn or_default(raw: &str) -> Vec<String> {
    parse(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's detection.never_detect is not valid; asking about every app");
        Vec::new()
    })
}

/// The list from `~/Meetings/.app/config.jsonc`, or none.
pub fn never_detect() -> Vec<String> {
    or_default(&super::raw_or_empty())
}

/// `raw` with the list written, comments and other keys kept.
pub fn with_never_detect(raw: &str, apps: &[String]) -> Result<String, ConfigError> {
    let list = tidy(apps.iter().cloned())
        .into_iter()
        .map(CstInputValue::String)
        .collect();
    with_section(raw, "detection", vec![(KEY, CstInputValue::Array(list))])
}

/// Save the list and return it as read back from disk.
pub fn set_never_detect(apps: &[String]) -> Result<Vec<String>, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    write_in(&dir, |raw| with_never_detect(raw, apps))?;
    parse(&read_in(&dir)?)
}

/// Add `app` to the saved list (once) and return the list as saved, all
/// under the config write lock, so a save from Settings cannot be lost.
pub fn add_never_detect(app: &str) -> Result<Vec<String>, ConfigError> {
    add_in(&super::app_dir().map_err(ConfigError::Root)?, app)
}

fn add_in(dir: &Path, app: &str) -> Result<Vec<String>, ConfigError> {
    write_in(dir, |raw| {
        let mut apps = parse(raw)?;
        apps.push(app.to_string());
        with_never_detect(raw, &apps)
    })?;
    parse(&read_in(dir)?)
}

#[cfg(test)]
mod tests {
    use super::super::file::SCHEMA;
    use super::*;

    #[test]
    fn missing_is_no_apps() {
        for raw in [
            "",
            "{}",
            r#"{ "detection": {} }"#,
            r#"{ "detection": { "processes": false } }"#,
        ] {
            assert!(parse(raw).unwrap().is_empty(), "{raw:?}");
        }
    }

    #[test]
    fn reads_the_list_tidied() {
        let raw = r#"{ "detection": { "never_detect": ["WhatsApp", " whatsapp ", "", "net.example.App"] } }"#;
        assert_eq!(parse(raw).unwrap(), ["WhatsApp", "net.example.App"]);
    }

    #[test]
    fn a_bad_value_is_an_error_and_the_reader_asks_about_every_app() {
        for raw in [
            r#"{ "detection": { "never_detect": "WhatsApp" } }"#,
            r#"{ "detection": { "never_detect": [1, 2] } }"#,
            "{ not json",
        ] {
            assert!(parse(raw).is_err(), "{raw:?}");
            assert!(or_default(raw).is_empty(), "{raw:?}");
        }
    }

    #[test]
    fn saving_keeps_the_switches_and_comments_and_round_trips() {
        let raw = r#"{
  // mine
  "detection": { "processes": false }
}"#;
        let apps = vec!["WhatsApp".to_string(), "Google Chrome".to_string()];
        let written = with_never_detect(raw, &apps).unwrap();
        assert!(written.contains("// mine"), "{written}");
        assert_eq!(parse(&written).unwrap(), apps);
        assert!(!super::super::detection_section::parse_detection(&written).processes);
        // And the switches' writer keeps the list.
        let switches = super::super::detection_section::with_detection(
            &written,
            &super::super::DetectionConfig::default(),
        )
        .unwrap();
        assert_eq!(parse(&switches).unwrap(), apps);
        let emptied = with_never_detect(&switches, &[]).unwrap();
        assert!(parse(&emptied).unwrap().is_empty());
        assert_eq!(emptied.matches(KEY).count(), 1, "{emptied}");
    }

    #[test]
    fn adding_on_disk_keeps_one_of_each() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        let add = |app: &str| add_in(&dir, app).unwrap();
        assert_eq!(add("WhatsApp"), ["WhatsApp"]);
        assert_eq!(add("Zoom"), ["WhatsApp", "Zoom"]);
        assert_eq!(add("whatsapp"), ["WhatsApp", "Zoom"]);
    }

    #[test]
    fn the_schema_documents_the_key_and_its_default() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let key = &schema["properties"]["detection"]["properties"][KEY];
        assert_eq!(key["type"], "array");
        assert_eq!(key["items"]["type"], "string");
        assert_eq!(key["default"], serde_json::json!([]));
    }
}
