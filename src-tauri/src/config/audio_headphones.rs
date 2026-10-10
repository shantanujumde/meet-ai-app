//! `audio.warn_no_headphones` (TUR-65): show the "No headphones" banner while
//! recording through speakers. On by default.
//!
//! Like `audio_mic`, a bad value is logged and the default is used: this
//! only shows a banner, so a typo must never stop a recording.

use serde::Deserialize;

use super::section::Flag;

/// The default when the key is missing or not valid.
pub const DEFAULT: bool = true;

/// Only this key of `audio`; the others are ignored here.
#[derive(Debug, Default, Deserialize)]
struct RawAudioHeadphones {
    warn_no_headphones: Option<bool>,
}

/// `audio.warn_no_headphones`, read strictly (TUR-176: `section::Flag`).
const FLAG: Flag<RawAudioHeadphones> = Flag {
    section: "audio",
    key: "warn_no_headphones",
    default: DEFAULT,
    pick: |section| section.warn_no_headphones,
    when_invalid: "audio section is not valid; warning without headphones (the default)",
};

/// The setting from `~/Meetings/.app/config.jsonc`, or the default.
pub fn warn_no_headphones() -> bool {
    FLAG.get()
}

#[cfg(test)]
mod tests {
    use super::super::error::ConfigError;
    use super::super::file::SCHEMA;
    use super::*;

    // The names these tests were written against (TUR-176 moved the code
    // into `section::Flag`).
    fn parse(raw: &str) -> Result<bool, ConfigError> {
        FLAG.parse(raw)
    }
    fn or_default(raw: &str) -> bool {
        FLAG.or_default(raw)
    }

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
