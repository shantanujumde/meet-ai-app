//! `audio.warn_no_headphones` (TUR-65): show the "No headphones" banner while
//! recording through speakers. On by default.
//!
//! Like `audio_mic`, a bad value is logged and the default is used: this
//! only shows a banner, so a typo must never stop a recording.

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::read_section;

/// The default when the key is missing or not valid.
pub const DEFAULT: bool = true;

/// Only this key of `audio`; the others are ignored here.
#[derive(Debug, Default, Deserialize)]
struct RawAudioHeadphones {
    warn_no_headphones: Option<bool>,
}

/// The setting from the text of `config.jsonc`.
pub fn parse(raw: &str) -> Result<bool, ConfigError> {
    let section: RawAudioHeadphones = read_section(raw, "audio")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    Ok(section.warn_no_headphones.unwrap_or(DEFAULT))
}

fn or_default(raw: &str) -> bool {
    parse(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's audio section is not valid; warning without headphones (the default)");
        DEFAULT
    })
}

/// The setting from `~/Meetings/.app/config.jsonc`, or the default.
pub fn warn_no_headphones() -> bool {
    or_default(&super::raw_or_empty())
}

#[cfg(test)]
mod tests {
    use super::super::file::SCHEMA;
    use super::*;

    #[test]
    fn missing_is_on() {
        for raw in [
            "",
            "{}",
            r#"{ "audio": {} }"#,
            r#"{ "audio": { "retention_days": 3 } }"#,
        ] {
            assert!(parse(raw).unwrap(), "{raw:?}");
        }
    }

    #[test]
    fn false_turns_it_off() {
        assert!(!parse(r#"{ "audio": { "warn_no_headphones": false } }"#).unwrap());
    }

    #[test]
    fn a_bad_value_is_an_error_and_the_reader_uses_the_default() {
        for raw in [
            r#"{ "audio": { "warn_no_headphones": "no" } }"#,
            "{ not json",
        ] {
            assert!(parse(raw).is_err(), "{raw:?}");
            assert!(or_default(raw), "{raw:?}");
        }
    }

    #[test]
    fn the_schema_documents_the_key_and_its_default() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let key = &schema["properties"]["audio"]["properties"]["warn_no_headphones"];
        assert_eq!(key["type"], "boolean");
        assert_eq!(key["default"], DEFAULT);
        assert!(
            !key["description"]
                .as_str()
                .unwrap()
                .contains("Not read by the app"),
            "{key}"
        );
    }
}
