//! `audio.use_builtin_mic_with_bluetooth` (TUR-91): record the Mac's own mic
//! when the default input is Bluetooth headphones, so they stay in playback
//! mode. On by default.
//!
//! Like `app`, a bad value is logged and the default is used: this only picks
//! a microphone, so a typo must never stop a recording.

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::read_section;

/// The key in the `audio` section.
const KEY: &str = "use_builtin_mic_with_bluetooth";

/// The default when the key is missing or not valid.
pub const DEFAULT: bool = true;

/// Only this key of `audio`; the others (`retention_days`, ...) are ignored
/// here.
#[derive(Debug, Default, Deserialize)]
struct RawAudioMic {
    use_builtin_mic_with_bluetooth: Option<bool>,
}

/// The setting from the text of `config.jsonc`.
pub fn parse(raw: &str) -> Result<bool, ConfigError> {
    let section: RawAudioMic = read_section(raw, "audio")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    Ok(section.use_builtin_mic_with_bluetooth.unwrap_or(DEFAULT))
}

fn or_default(raw: &str) -> bool {
    parse(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's audio section is not valid; using the Mac's own mic with Bluetooth (the default)");
        DEFAULT
    })
}

/// The setting from `~/Meetings/.app/config.jsonc`, or the default.
pub fn use_builtin_mic_with_bluetooth() -> bool {
    or_default(&super::raw_or_empty())
}

/// `raw` with the setting written, comments and other keys kept.
pub fn with_setting(raw: &str, on: bool) -> Result<String, ConfigError> {
    with_section(raw, "audio", vec![(KEY, on.into())])
}

/// Save the setting and return it as read back from disk.
pub fn set_use_builtin_mic_with_bluetooth(on: bool) -> Result<bool, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    write_in(&dir, |raw| with_setting(raw, on))?;
    parse(&read_in(&dir)?)
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
    fn a_bad_value_is_an_error_and_the_reader_uses_the_default() {
        for raw in [
            r#"{ "audio": { "use_builtin_mic_with_bluetooth": "no" } }"#,
            "{ not json",
        ] {
            assert!(parse(raw).is_err(), "{raw:?}");
            assert!(or_default(raw), "{raw:?}");
        }
    }

    #[test]
    fn saving_keeps_retention_and_comments() {
        let raw = r#"{
  // mine
  "audio": { "retention_days": 30 }
}"#;
        let off = with_setting(raw, false).unwrap();
        assert!(off.contains("// mine"), "{off}");
        assert!(off.contains(r#""retention_days": 30"#), "{off}");
        assert!(!parse(&off).unwrap());
        let on = with_setting(&off, true).unwrap();
        assert!(parse(&on).unwrap());
        assert_eq!(on.matches(KEY).count(), 1, "{on}");
    }

    #[test]
    fn the_schema_documents_the_key_and_its_default() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let key = &schema["properties"]["audio"]["properties"][KEY];
        assert_eq!(key["type"], "boolean");
        assert_eq!(key["default"], DEFAULT);
    }
}
