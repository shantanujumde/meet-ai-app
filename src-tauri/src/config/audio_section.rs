//! The `audio` section of `config.jsonc` (SPEC §3.5): how long recorded
//! audio is kept (L16, TUR-45).
//!
//! Only `retention_days` is read. Like `transcription` and `detection`, a bad
//! value is logged and the default (7 days) is used, so startup never fails
//! on it ([`audio`]); [`parse_audio`] returns the error for a caller that
//! wants it.

use serde::Deserialize;
use store::retention::Retention;

use super::agent_section::ConfigError;
use super::read_section;

/// `audio` in `config.jsonc`, as far as the app reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioConfig {
    /// `retention_days`: 7 by default, `0` deletes audio once the transcript
    /// is done, `-1` keeps it forever.
    pub retention: Retention,
}

/// `audio` as written. A missing key is the default; a value that is not a
/// whole number fails the section.
#[derive(Debug, Default, Deserialize)]
struct RawAudio {
    retention_days: Option<i64>,
}

/// `audio` from the text of `config.jsonc`. Empty text, or no `audio` key, is
/// the defaults. A negative number other than `-1` is an error.
pub fn parse_audio(raw: &str) -> Result<AudioConfig, ConfigError> {
    let audio: RawAudio = read_section(raw, "audio")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    let retention = match audio.retention_days {
        None => Retention::default(),
        Some(days) => Retention::from_days(days).ok_or_else(|| {
            ConfigError::Invalid(format!(
                "audio: retention_days is {days}; use -1 (keep forever), 0 (delete once \
                 transcribed) or a number of days"
            ))
        })?,
    };
    Ok(AudioConfig { retention })
}

/// [`parse_audio`], with a bad section logged and replaced by the defaults.
fn audio_or_defaults(raw: &str) -> AudioConfig {
    parse_audio(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's audio section is not valid; keeping audio for the default 7 days");
        AudioConfig::default()
    })
}

/// `audio` from `~/Meetings/.app/config.jsonc`, or the SPEC §3.5 defaults if
/// the file or section is missing or not valid (logged).
pub fn audio() -> AudioConfig {
    audio_or_defaults(&super::raw_or_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_section_keeps_audio_for_seven_days() {
        for raw in [
            "",
            "{}",
            r#"{ "audio": {} }"#,
            r#"{ "audio": { "warn_no_headphones": false } }"#,
        ] {
            assert_eq!(
                parse_audio(raw).unwrap().retention,
                Retention::Days(7),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn the_spec_values_read_back() {
        let days = |raw: &str| parse_audio(raw).unwrap().retention;
        assert_eq!(
            days(r#"{ "audio": { "retention_days": 0 } }"#),
            Retention::Days(0)
        );
        assert_eq!(
            days(r#"{ "audio": { "retention_days": -1 } }"#),
            Retention::KeepForever
        );
        assert_eq!(
            days(
                r#"{
                    // JSONC
                    "audio": { "retention_days": 30, "warn_no_headphones": true }
                }"#
            ),
            Retention::Days(30)
        );
    }

    #[test]
    fn a_bad_value_is_an_error_and_the_app_falls_back_to_seven_days() {
        for raw in [
            r#"{ "audio": { "retention_days": -2 } }"#,
            r#"{ "audio": { "retention_days": 2.5 } }"#,
            r#"{ "audio": { "retention_days": "7" } }"#,
            r#"{ "audio": null }"#,
            "{ not json",
        ] {
            assert!(parse_audio(raw).is_err(), "{raw:?}");
            assert_eq!(audio_or_defaults(raw), AudioConfig::default(), "{raw:?}");
        }
    }

    #[test]
    fn a_null_retention_is_the_default() {
        let raw = r#"{ "audio": { "retention_days": null } }"#;
        assert_eq!(parse_audio(raw).unwrap(), AudioConfig::default());
    }
}
