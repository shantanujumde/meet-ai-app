//! The `audio` section of `config.jsonc` (SPEC §3.5): how long recorded
//! audio is kept (L16, TUR-45).
//!
//! Only `retention_days` is read, and only by a job that deletes files, so
//! unlike `transcription` and `detection` a doubt never falls back to the
//! default (TUR-85). [`retention_policy`] decides: the default 7 days applies
//! only when the file or the key is absent. A file that cannot be read (a
//! permission error, a dataless sync placeholder), one that does not parse
//! (a typo in any section), or an `audio` section that is not valid pauses
//! the job instead, and nothing is deleted until it is fixed.

use serde::Deserialize;
use store::retention::Retention;

use super::agent_section::ConfigError;
use super::read_section;

/// `audio` as written. A missing (or `null`) key is the default; a value
/// that is not a whole number fails the section.
#[derive(Debug, Default, Deserialize)]
pub struct RawAudioSection {
    retention_days: Option<i64>,
}

/// What the retention job should do, given `config.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Policy {
    /// Run with this retention.
    Run(Retention),
    /// Do not delete anything: the config could not be trusted. The reason
    /// is a sentence for the log and Settings.
    Pause(String),
}

/// The retention job's decision from what reading `audio` gave: `Ok(None)`
/// when the file or the `audio` key is absent, `Ok(Some(_))` for the section
/// as written, `Err` when the file could not be read or parsed.
pub fn retention_policy(read: Result<Option<RawAudioSection>, ConfigError>) -> Policy {
    let section = match read {
        Ok(section) => section.unwrap_or_default(),
        Err(error) => return Policy::Pause(error.to_string()),
    };
    match section.retention_days {
        None => Policy::Run(Retention::default()),
        Some(days) => match Retention::from_days(days) {
            Some(retention) => Policy::Run(retention),
            None => Policy::Pause(
                ConfigError::Invalid(format!(
                    "audio: retention_days is {days}; use -1 (keep forever), 0 (delete once \
                     transcribed) or a number of days"
                ))
                .to_string(),
            ),
        },
    }
}

/// `audio` from the text of `config.jsonc`. Empty text, or no `audio` key,
/// is `Ok(None)`.
fn parse_raw(raw: &str) -> Result<Option<RawAudioSection>, ConfigError> {
    read_section(raw, "audio").map_err(ConfigError::Invalid)
}

/// `audio` from `~/Meetings/.app/config.jsonc`. A file that is not there is
/// `Ok(None)`; any other read error, or no meetings folder to look in, is an
/// `Err`.
fn read_raw() -> Result<Option<RawAudioSection>, ConfigError> {
    let path = super::app_dir()
        .map_err(ConfigError::Root)?
        .join(super::FILE);
    read_raw_at(&path)
}

/// [`read_raw`] for the `config.jsonc` at `path`.
fn read_raw_at(path: &std::path::Path) -> Result<Option<RawAudioSection>, ConfigError> {
    match super::once::read(path) {
        Ok(raw) => parse_raw(&raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::Io(error)),
    }
}

/// The retention job's [`Policy`] from `~/Meetings/.app/config.jsonc`, read
/// now. A pause is logged here, once per read.
pub fn audio() -> Policy {
    let policy = retention_policy(read_raw());
    if let Policy::Pause(reason) = &policy {
        tracing::warn!(%reason, "audio cleanup paused: config.jsonc could not be read");
    }
    policy
}

/// [`audio`] for the `config.jsonc` at `path`, unlogged, for the retention
/// job's own tests.
#[cfg(test)]
pub(crate) fn policy_at(path: &std::path::Path) -> Policy {
    retention_policy(read_raw_at(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(raw: &str) -> Policy {
        retention_policy(parse_raw(raw))
    }

    #[test]
    fn an_absent_file_or_key_keeps_audio_for_seven_days() {
        assert_eq!(retention_policy(Ok(None)), Policy::Run(Retention::Days(7)));
        for raw in [
            "",
            "{}",
            r#"{ "audio": {} }"#,
            r#"{ "audio": { "warn_no_headphones": false } }"#,
            r#"{ "audio": { "retention_days": null } }"#,
        ] {
            assert_eq!(policy(raw), Policy::Run(Retention::Days(7)), "{raw:?}");
        }
    }

    #[test]
    fn the_spec_values_read_back() {
        assert_eq!(
            policy(r#"{ "audio": { "retention_days": 0 } }"#),
            Policy::Run(Retention::Days(0))
        );
        assert_eq!(
            policy(r#"{ "audio": { "retention_days": -1 } }"#),
            Policy::Run(Retention::KeepForever)
        );
        assert_eq!(
            policy(
                r#"{
                    // JSONC
                    "audio": { "retention_days": 30, "warn_no_headphones": true }
                }"#
            ),
            Policy::Run(Retention::Days(30))
        );
    }

    #[test]
    fn an_invalid_audio_section_pauses_rather_than_falling_back_to_seven_days() {
        for raw in [
            r#"{ "audio": { "retention_days": -2 } }"#,
            r#"{ "audio": { "retention_days": 2.5 } }"#,
            r#"{ "audio": { "retention_days": "7" } }"#,
            r#"{ "audio": null }"#,
        ] {
            assert!(
                matches!(policy(raw), Policy::Pause(ref reason) if reason.contains("audio")),
                "{raw:?}: {:?}",
                policy(raw)
            );
        }
    }

    #[test]
    fn a_file_that_does_not_parse_pauses_even_for_a_typo_elsewhere() {
        for raw in [
            "{ not json",
            r#"{ "audio": { "retention_days": -1 }, "agent": { "harness": "claude", } ,, }"#,
        ] {
            assert!(
                matches!(policy(raw), Policy::Pause(_)),
                "{raw:?}: {:?}",
                policy(raw)
            );
        }
    }

    #[test]
    fn on_disk_only_a_missing_file_is_the_default() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.jsonc");
        assert_eq!(
            retention_policy(read_raw_at(&path)),
            Policy::Run(Retention::Days(7))
        );

        // Something there that cannot be read as text: a folder in its place
        // fails the read on every OS, the way a permission error does.
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(
            retention_policy(read_raw_at(&path)),
            Policy::Pause(_)
        ));
        std::fs::remove_dir(&path).unwrap();

        std::fs::write(&path, r#"{ "audio": { "retention_days": 3 } }"#).unwrap();
        assert_eq!(
            retention_policy(read_raw_at(&path)),
            Policy::Run(Retention::Days(3))
        );
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(
            retention_policy(read_raw_at(&path)),
            Policy::Pause(_)
        ));
    }

    #[test]
    fn an_unreadable_file_or_meetings_folder_pauses() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(matches!(
            retention_policy(Err(ConfigError::Io(denied))),
            Policy::Pause(_)
        ));
        let no_root = crate::error::UiError::app("no-home-dir", "no home folder");
        assert!(matches!(
            retention_policy(Err(ConfigError::Root(no_root))),
            Policy::Pause(ref reason) if reason.contains("no home folder")
        ));
    }
}
