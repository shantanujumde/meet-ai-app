//! Round trips for the `agent` and `tickets` sections (TUR-3).

use std::path::PathBuf;

use super::FILE;
use super::agent_section::{AgentConfig, Harness, TicketsConfig, parse_agent, parse_tickets};
use super::error::ConfigError;
use super::file::{SCHEMA, SCHEMA_FILE, with_agent, with_tickets};

fn every_field_set() -> AgentConfig {
    AgentConfig {
        harness: Harness::Codex,
        model: Some("gpt-5-codex".into()),
        binary_path: Some(PathBuf::from("/opt/homebrew/bin/codex")),
        auto_run: false,
        timeout_sec: 120,
    }
}

#[test]
fn a_missing_file_or_section_is_the_a11_defaults() {
    let defaults = AgentConfig {
        harness: Harness::ClaudeCode,
        model: None,
        binary_path: None,
        auto_run: true,
        timeout_sec: 300,
    };
    assert_eq!(AgentConfig::default(), defaults);
    for raw in ["", "// only a comment\n", "{}", r#"{ "agent": {} }"#] {
        assert_eq!(parse_agent(raw).unwrap(), defaults, "{raw:?}");
        assert_eq!(parse_tickets(raw).unwrap(), TicketsConfig::default());
    }
    assert_eq!(TicketsConfig::default().tracker, "linear");
    assert_eq!(TicketsConfig::default().tracker_mcp, "claude.ai Linear");
}

#[test]
fn the_defaults_round_trip_through_an_empty_file() {
    let written = with_agent("", &AgentConfig::default()).unwrap();
    assert_eq!(parse_agent(&written).unwrap(), AgentConfig::default());
    assert!(written.contains(r#""$schema": "./config.schema.json""#));
    // `null`, not a missing key, so the file shows the user the key exists.
    assert!(written.contains(r#""binary_path": null"#));
    // A new install stores no model: the CLI picks (A14).
    assert!(written.contains(r#""model": null"#), "{written}");
}

#[test]
fn a_null_or_blank_model_is_the_cli_s_own_and_a_stored_one_is_kept() {
    for raw in [
        r#"{ "agent": { "model": null } }"#,
        r#"{ "agent": { "model": "" } }"#,
        r#"{ "agent": { "model": "   " } }"#,
    ] {
        assert_eq!(parse_agent(raw).unwrap().model, None, "{raw}");
    }
    // An existing config keeps the model it already stores.
    let raw = r#"{ "agent": { "model": "opus" } }"#;
    assert_eq!(parse_agent(raw).unwrap().model.as_deref(), Some("opus"));
    let written = with_agent(raw, &parse_agent(raw).unwrap()).unwrap();
    assert_eq!(
        parse_agent(&written).unwrap().model.as_deref(),
        Some("opus")
    );
}

#[test]
fn every_field_set_round_trips() {
    let agent = every_field_set();
    let tickets = TicketsConfig {
        tracker: "jira".into(),
        tracker_mcp: "claude.ai Atlassian".into(),
    };
    let written = with_tickets(&with_agent("{}", &agent).unwrap(), &tickets).unwrap();
    assert_eq!(parse_agent(&written).unwrap(), agent);
    assert_eq!(parse_tickets(&written).unwrap(), tickets);
}

#[test]
fn every_harness_round_trips() {
    for harness in Harness::ALL {
        let agent = AgentConfig {
            harness,
            ..AgentConfig::default()
        };
        let written = with_agent("{}", &agent).unwrap();
        assert_eq!(parse_agent(&written).unwrap().harness, harness);
    }
}

#[test]
fn the_spec_3_5_example_parses() {
    let raw = r#"{
        "$schema": "./config.schema.json",
        "agent": {                          // A11
          "harness": "claude-code",         // "codex" | "none" (= copy-prompt fallback)
          "model": "opus",
          "binary_path": null,
          "auto_run": true,
          "timeout_sec": 300
        },
        "tickets": { "tracker": "linear", "tracker_mcp": "claude.ai Linear" },  // named in the Sync prompt
        "repos": { "default": "~/apps/api" }
    }"#;
    let stored = AgentConfig {
        model: Some("opus".into()),
        ..AgentConfig::default()
    };
    assert_eq!(parse_agent(raw).unwrap(), stored);
    assert_eq!(parse_tickets(raw).unwrap(), TicketsConfig::default());
}

#[test]
fn an_unknown_harness_is_an_error_not_a_silent_default() {
    let error = parse_agent(r#"{ "agent": { "harness": "codx" } }"#).unwrap_err();
    assert!(
        matches!(&error, ConfigError::UnknownHarness(name) if name == "codx"),
        "{error:?}"
    );
    let ui: crate::error::UiError = error.into();
    assert_eq!((ui.domain.as_str(), ui.kind), ("app", "unknown-harness"));
    assert!(ui.message.contains("codx"), "{}", ui.message);
}

#[test]
fn wrong_types_and_a_zero_timeout_are_errors() {
    for raw in [
        r#"{ "agent": { "timeout_sec": 0 } }"#,
        r#"{ "agent": { "timeout_sec": -5 } }"#,
        r#"{ "agent": { "timeout_sec": 300.5 } }"#,
        r#"{ "agent": { "timeout_sec": "five minutes" } }"#,
        r#"{ "agent": { "auto_run": "yes" } }"#,
        r#"{ "agent": { "harness": 1 } }"#,
        "{ not json",
    ] {
        assert!(
            matches!(parse_agent(raw), Err(ConfigError::Invalid(_))),
            "{raw}"
        );
    }
}

#[test]
fn a_whole_float_timeout_and_an_empty_binary_path_read_like_the_schema_allows() {
    let agent = parse_agent(r#"{ "agent": { "timeout_sec": 300.0, "binary_path": "" } }"#).unwrap();
    assert_eq!(agent.timeout_sec, 300);
    assert_eq!(agent.binary_path, None);

    let written = with_agent(
        "{}",
        &AgentConfig {
            binary_path: Some(PathBuf::new()),
            ..AgentConfig::default()
        },
    )
    .unwrap();
    assert!(written.contains(r#""binary_path": null"#), "{written}");
}

#[test]
fn a_bad_value_in_one_section_never_breaks_reading_the_other() {
    let raw = r#"{ "agent": { "harness": "codex" }, "tickets": { "tracker": 5 } }"#;
    assert_eq!(parse_agent(raw).unwrap().harness, Harness::Codex);
    assert!(matches!(parse_tickets(raw), Err(ConfigError::Invalid(_))));

    let raw = r#"{ "agent": { "harness": "codx" }, "tickets": { "tracker": "jira" },
                   "transcription": { "engine": "whisper" } }"#;
    assert!(matches!(
        parse_agent(raw),
        Err(ConfigError::UnknownHarness(_))
    ));
    assert_eq!(parse_tickets(raw).unwrap().tracker, "jira");
    assert_eq!(super::parse(raw).engine, stt::registry::Preference::Whisper);
}

#[test]
fn a_section_that_is_not_an_object_is_invalid_and_a_save_replaces_it() {
    for raw in [
        r#"{ "agent": null }"#,
        r#"{ "agent": "x" }"#,
        r#"{ "agent": [1] }"#,
    ] {
        assert!(
            matches!(parse_agent(raw), Err(ConfigError::Invalid(_))),
            "{raw}"
        );
        let written = with_agent(raw, &every_field_set()).unwrap();
        assert_eq!(parse_agent(&written).unwrap(), every_field_set(), "{raw}");
    }
}

#[test]
fn a_top_level_null_reads_as_defaults_but_is_never_overwritten() {
    assert_eq!(parse_agent("null").unwrap(), AgentConfig::default());
    assert!(matches!(
        with_agent("null", &AgentConfig::default()),
        Err(ConfigError::Invalid(_))
    ));
}

#[test]
fn a_missing_meetings_folder_keeps_its_own_ui_error_kind() {
    let error = ConfigError::Root(crate::error::UiError::app("no-home-dir", "no home folder"));
    let ui: crate::error::UiError = error.into();
    assert_eq!((ui.domain.as_str(), ui.kind), ("app", "no-home-dir"));
    assert_eq!(ui.message, "no home folder");
}

#[test]
fn a_write_keeps_comments_unknown_keys_and_other_sections() {
    let raw = r#"// my meet-ai settings
{
  "$schema": "./config.schema.json",
  "transcription": { "engine": "whisper" }, // faster on this Mac
  "agent": {
    "harness": "claude-code", // the one I pay for
    "future_key": [1, 2]
  },
  "something_new": { "kept": true }
}
"#;
    let written = with_agent(raw, &every_field_set()).unwrap();
    for kept in [
        "// my meet-ai settings",
        "// faster on this Mac",
        "// the one I pay for",
        r#""future_key": [1, 2]"#,
        r#""something_new": { "kept": true }"#,
        r#""transcription": { "engine": "whisper" }"#,
    ] {
        assert!(written.contains(kept), "lost {kept:?} in:\n{written}");
    }
    assert_eq!(parse_agent(&written).unwrap(), every_field_set());
    assert_eq!(
        super::parse(&written).engine,
        stt::registry::Preference::Whisper
    );
}

#[test]
fn a_write_fixes_an_unknown_harness() {
    let written = with_agent(
        r#"{ "agent": { "harness": "codx" } }"#,
        &AgentConfig::default(),
    )
    .unwrap();
    assert_eq!(parse_agent(&written).unwrap(), AgentConfig::default());
}

#[test]
fn a_file_that_does_not_parse_is_refused_not_overwritten() {
    for raw in ["{ not json", "[1, 2]"] {
        assert!(
            matches!(
                with_agent(raw, &AgentConfig::default()),
                Err(ConfigError::Invalid(_))
            ),
            "{raw}"
        );
    }
}

#[test]
fn saving_to_disk_round_trips_and_writes_the_schema_beside_it() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join(".app");
    assert_eq!(
        super::file::read_in(&dir).unwrap(),
        "",
        "no file reads as empty"
    );

    super::file::write_in(&dir, |raw| with_agent(raw, &every_field_set())).unwrap();
    super::file::write_in(&dir, |raw| with_tickets(raw, &TicketsConfig::default())).unwrap();

    let raw = super::file::read_in(&dir).unwrap();
    assert_eq!(parse_agent(&raw).unwrap(), every_field_set());
    assert_eq!(parse_tickets(&raw).unwrap(), TicketsConfig::default());
    assert_eq!(
        std::fs::read_to_string(dir.join(SCHEMA_FILE)).unwrap(),
        SCHEMA
    );
    // No temp file left behind: only the two files the save is for.
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, [FILE, SCHEMA_FILE]);
}

#[test]
fn a_refused_write_leaves_the_file_alone() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(FILE);
    std::fs::write(&path, "{ not json").unwrap();
    let result = super::file::write_in(temp.path(), |raw| with_agent(raw, &every_field_set()));
    assert!(matches!(result, Err(ConfigError::Invalid(_))));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

fn schema() -> serde_json::Value {
    serde_json::from_str(SCHEMA).unwrap()
}

#[test]
fn the_schema_lists_exactly_the_harnesses_the_code_knows() {
    let schema = schema();
    let listed = &schema["properties"]["agent"]["properties"]["harness"]["enum"];
    let known: Vec<_> = Harness::ALL.iter().map(|h| h.as_str()).collect();
    assert_eq!(listed, &serde_json::json!(known));
}

#[test]
fn the_schema_defaults_match_the_code_defaults() {
    let schema = schema();
    let agent = &schema["properties"]["agent"]["properties"];
    let defaults = AgentConfig::default();
    assert_eq!(agent["harness"]["default"], defaults.harness.as_str());
    assert_eq!(agent["model"]["default"], serde_json::json!(defaults.model));
    assert_eq!(agent["binary_path"]["default"], serde_json::Value::Null);
    assert_eq!(agent["auto_run"]["default"], defaults.auto_run);
    assert_eq!(agent["timeout_sec"]["default"], defaults.timeout_sec);

    let transcription = &schema["properties"]["transcription"]["properties"];
    assert_eq!(
        transcription["model"]["default"],
        crate::engine::DEFAULT_MODEL
    );
    assert_eq!(
        serde_json::from_value::<stt::registry::Preference>(
            transcription["engine"]["default"].clone()
        )
        .unwrap(),
        stt::registry::Preference::default()
    );
    assert_eq!(
        schema["properties"]["$schema"]["default"],
        format!("./{SCHEMA_FILE}")
    );
    // The root is found before this file can be (it lives under the root),
    // so offering the key would only mislead.
    assert!(schema["properties"].get("meetings_root").is_none());

    let tickets = &schema["properties"]["tickets"]["properties"];
    let defaults = TicketsConfig::default();
    assert_eq!(tickets["tracker"]["default"], defaults.tracker.as_str());
    assert_eq!(
        tickets["tracker_mcp"]["default"],
        defaults.tracker_mcp.as_str()
    );
}

#[test]
fn the_schema_never_rejects_unknown_keys() {
    // The app keeps keys it does not know, so the schema must not flag them.
    fn walk(value: &serde_json::Value) {
        if let Some(object) = value.as_object() {
            assert_ne!(object.get("additionalProperties"), Some(&false.into()));
            object.values().for_each(walk);
        }
    }
    walk(&schema());
}

#[test]
fn repos_default_is_read_and_blank_is_unset() {
    use super::agent_section::parse_default_repo;
    let read = |raw: &str| parse_default_repo(raw).unwrap();
    assert_eq!(read(""), None);
    assert_eq!(read(r#"{ "repos": {} }"#), None);
    assert_eq!(read(r#"{ "repos": { "default": "  " } }"#), None);
    assert_eq!(
        read(r#"{ "repos": { "default": "~/apps/api" } } // comment"#),
        Some("~/apps/api".to_owned())
    );
    assert!(matches!(
        parse_default_repo(r#"{ "repos": { "default": 3 } }"#),
        Err(ConfigError::Invalid(_))
    ));
}

/// TUR-113: tickets go to a tracker on their own only once the user saved
/// one; the defaults alone never count.
#[test]
fn only_a_saved_tracker_counts_as_chosen() {
    use super::agent_section::parse_tickets_chosen;
    for raw in [
        "",
        "{}",
        r#"{ "tickets": {} }"#,
        r#"{ "tickets": { "tracker": "linear" } }"#,
    ] {
        assert!(!parse_tickets_chosen(raw).unwrap(), "{raw:?}");
    }
    let blank = r#"{ "tickets": { "tracker": "linear", "tracker_mcp": " " } }"#;
    assert!(!parse_tickets_chosen(blank).unwrap());
    let saved = with_tickets("", &TicketsConfig::default()).unwrap();
    assert!(parse_tickets_chosen(&saved).unwrap(), "{saved}");
    assert!(parse_tickets_chosen("{ not json").is_err());
}

/// The tracker Settings saves stays saved, and stays "chosen", on disk after
/// other sections are saved later (the owner's tracker went missing after a
/// save that never reached the file).
#[test]
fn a_saved_tracker_survives_later_saves_of_other_sections() {
    use super::agent_section::parse_tickets_chosen;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join(".app");
    let tickets = TicketsConfig {
        tracker: "linear".into(),
        tracker_mcp: "claude.ai Linear".into(),
    };

    super::file::write_in(&dir, |raw| with_tickets(raw, &tickets)).unwrap();
    super::file::write_in(&dir, |raw| with_agent(raw, &every_field_set())).unwrap();
    super::file::write_in(&dir, |raw| {
        super::file::with_section(raw, "transcription", vec![("language", "mr".into())])
    })
    .unwrap();

    let raw = super::file::read_in(&dir).unwrap();
    assert_eq!(parse_tickets(&raw).unwrap(), tickets);
    assert!(parse_tickets_chosen(&raw).unwrap(), "{raw}");
}
