//! Writing the `transcription` section from the Settings engine picker
//! (TUR-75): both keys change, nothing else in the file does.

use stt::registry::Preference;

use super::file::SCHEMA;
use super::file::{read_in, with_agent, with_transcription, write_in};
use super::parse;
use super::{AgentConfig, Transcription};

const COMMENTED: &str = r#"{
  "$schema": "./config.schema.json",
  // How speech becomes text. I set this by hand once.
  "transcription": {
    "engine": "auto",              // or "apple-speech" | "whisper"
    "model": "large-v3-turbo-q5_0",
    "language": "en",
    "live": true
  },
  /* Notes */
  "agent": { "harness": "codex", "model": "gpt-5-codex" },
  "mystery": [1, 2, 3]
}
"#;

#[test]
fn writing_both_keys_keeps_comments_other_keys_and_other_sections() {
    let written = with_transcription(COMMENTED, Preference::Whisper, "small.en-q5_1").unwrap();

    // Only the two values changed: swap them back and the file is
    // byte-for-byte what it was.
    let restored = written
        .replace(r#""engine": "whisper""#, r#""engine": "auto""#)
        .replace(
            r#""model": "small.en-q5_1""#,
            r#""model": "large-v3-turbo-q5_0""#,
        );
    assert_eq!(restored, COMMENTED);

    assert_eq!(
        parse(&written),
        Transcription {
            engine: Preference::Whisper,
            model: "small.en-q5_1".into(),
            language: "en".into(),
            live: true,
        }
    );
    assert!(written.contains("// How speech becomes text. I set this by hand once."));
    assert!(written.contains(r#"// or "apple-speech" | "whisper""#));
    assert!(written.contains(r#""language": "en""#));
    assert!(written.contains(r#""mystery": [1, 2, 3]"#));
}

#[test]
fn the_schema_lists_every_engine_the_registry_reads() {
    // TUR-62: "parakeet" is a valid value, so an editor's schema check must
    // not flag it.
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let listed = &schema["properties"]["transcription"]["properties"]["engine"]["enum"];
    let known: Vec<_> = [
        Preference::Auto,
        Preference::AppleSpeech,
        Preference::Whisper,
        Preference::Parakeet,
    ]
    .into_iter()
    .map(|engine| serde_json::to_value(engine).unwrap())
    .collect();
    assert_eq!(listed, &serde_json::json!(known));
    let parakeet = with_transcription("", Preference::Parakeet, "small.en-q5_1").unwrap();
    assert!(
        parakeet.contains(&format!("\"{}\"", stt::registry::PARAKEET)),
        "{parakeet}"
    );
}

#[test]
fn the_schema_documents_the_language_and_its_default() {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let key = &schema["properties"]["transcription"]["properties"]["language"];
    assert_eq!(key["type"], "string");
    assert_eq!(key["default"], stt::languages::AUTO);
    assert_eq!(key["default"], super::Transcription::default().language);
    assert!(
        !key["description"]
            .as_str()
            .unwrap()
            .contains("Not read by the app"),
        "{key}"
    );
}

#[test]
fn every_engine_round_trips() {
    for engine in [
        Preference::Auto,
        Preference::AppleSpeech,
        Preference::Whisper,
        Preference::Parakeet,
    ] {
        let written = with_transcription("", engine, "small.en-q5_1").unwrap();
        assert_eq!(parse(&written).engine, engine, "{written}");
    }
    // The names written are the registry's own (TUR-90: no second list here).
    let apple = with_transcription("", Preference::AppleSpeech, "small.en-q5_1").unwrap();
    assert!(
        apple.contains(&format!("\"{}\"", stt::registry::APPLE_SPEECH)),
        "{apple}"
    );
    let whisper = with_transcription("", Preference::Whisper, "small.en-q5_1").unwrap();
    assert!(
        whisper.contains(&format!("\"{}\"", stt::registry::WHISPER)),
        "{whisper}"
    );
}

#[test]
fn a_new_file_gets_the_schema_line_and_the_section() {
    let written = with_transcription("", Preference::AppleSpeech, "large-v3-turbo-q5_0").unwrap();
    assert!(written.contains(r#""$schema": "./config.schema.json""#));
    assert_eq!(parse(&written).engine, Preference::AppleSpeech);
}

#[test]
fn a_file_without_the_section_gains_it_and_keeps_the_agent() {
    let raw = with_agent("", &AgentConfig::default()).unwrap();
    let written = with_transcription(&raw, Preference::Whisper, "small.en-q5_1").unwrap();
    assert_eq!(
        super::agent_section::parse_agent(&written).unwrap(),
        AgentConfig::default()
    );
    assert_eq!(parse(&written).model, "small.en-q5_1");
}

#[test]
fn a_file_that_does_not_parse_is_refused_and_left_alone() {
    let temp = tempfile::tempdir().unwrap();
    let broken = "{ \"transcription\": ";
    std::fs::write(temp.path().join(super::FILE), broken).unwrap();
    let result = write_in(temp.path(), |raw| {
        with_transcription(raw, Preference::Whisper, "small.en-q5_1")
    });
    assert!(result.is_err());
    assert_eq!(read_in(temp.path()).unwrap(), broken);
}

#[test]
fn saving_to_disk_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join(super::FILE), COMMENTED).unwrap();
    write_in(temp.path(), |raw| {
        with_transcription(raw, Preference::Whisper, "small.en-q5_1")
    })
    .unwrap();
    let raw = read_in(temp.path()).unwrap();
    assert_eq!(parse(&raw).engine, Preference::Whisper);
    assert!(raw.contains("/* Notes */"));
}

#[test]
fn the_schema_says_repos_default_is_read() {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let repos = &schema["properties"]["repos"];
    let key = &repos["properties"]["default"];
    assert_eq!(key["type"], "string");
    for text in [&repos["description"], &key["description"]] {
        let text = text.as_str().unwrap();
        assert!(!text.contains("Not read by the app"), "{text}");
        assert!(!text.contains("no effect"), "{text}");
    }
    assert!(key["description"].as_str().unwrap().contains("Start Work"));
}

#[test]
fn live_is_read_and_defaults_to_on() {
    // TUR-137: `live: false` is how a user asks for transcription after Stop.
    assert!(parse("").live);
    assert!(parse(r#"{ "transcription": { "engine": "whisper" } }"#).live);
    assert!(parse(r#"{ "transcription": { "live": true } }"#).live);
    let off = parse(r#"{ "transcription": { "live": false, "model": "small.en-q5_1" } }"#);
    assert!(!off.live);
    assert_eq!(off.model, "small.en-q5_1", "the other keys still read");
    // The commented example the app writes carries it too.
    assert!(parse(COMMENTED).live);
}

#[test]
fn the_schema_says_live_is_read() {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let key = &schema["properties"]["transcription"]["properties"]["live"];
    assert_eq!(key["type"], "boolean");
    assert_eq!(key["default"], true);
    let text = key["description"].as_str().unwrap();
    assert!(!text.contains("Not read by the app"), "{text}");
    assert!(!text.contains("no effect"), "{text}");
}
