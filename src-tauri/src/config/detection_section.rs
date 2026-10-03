//! The `detection` section of `config.jsonc` (SPEC §3.5): which signals the
//! app uses to notice that a meeting has started.
//!
//! Each of the three switches turns one signal off with a config edit only.
//! Like `transcription`, a bad section is logged and the app runs with the
//! defaults ([`detection`]); [`parse_detection`] returns the error for a
//! caller that wants to show it.
//!
//! There is no detection loop on main yet: TUR-27 calls [`detection`].

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::read_section;

/// `detection` in `config.jsonc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectionConfig {
    /// Use calendar events to spot meetings.
    pub calendar: bool,
    /// Watch for running meeting apps, such as Zoom or Teams.
    pub processes: bool,
    /// Watch for the mic and speakers being used at the same time.
    pub audio_activity: bool,
    /// A calendar event counts as a meeting only with at least this many
    /// attendees.
    pub min_attendees: u32,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            calendar: true,
            processes: true,
            audio_activity: true,
            min_attendees: 2,
        }
    }
}

/// `detection` as written. Every key optional, so a missing one is the default.
#[derive(Debug, Default, Deserialize)]
struct RawDetection {
    calendar: Option<bool>,
    processes: Option<bool>,
    audio_activity: Option<bool>,
    min_attendees: Option<u32>,
}

/// `detection` from the text of `config.jsonc`. Empty text, or no
/// `detection` key, is all defaults.
pub fn parse_detection(raw: &str) -> Result<DetectionConfig, ConfigError> {
    let detection: RawDetection = read_section(raw, "detection")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    let defaults = DetectionConfig::default();
    Ok(DetectionConfig {
        calendar: detection.calendar.unwrap_or(defaults.calendar),
        processes: detection.processes.unwrap_or(defaults.processes),
        audio_activity: detection.audio_activity.unwrap_or(defaults.audio_activity),
        min_attendees: detection.min_attendees.unwrap_or(defaults.min_attendees),
    })
}

/// [`parse_detection`], with a bad section logged and replaced by the
/// defaults: startup never fails on it.
fn detection_or_defaults(raw: &str) -> DetectionConfig {
    parse_detection(raw).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's detection section is not valid; using defaults");
        DetectionConfig::default()
    })
}

/// `detection` from `~/Meetings/.app/config.jsonc`, or the SPEC §3.5
/// defaults if the file or section is missing or not valid (logged).
#[allow(dead_code)] // TUR-27 (detection loop) calls this.
pub fn detection() -> DetectionConfig {
    detection_or_defaults(&super::raw_or_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_section_is_the_spec_3_5_defaults() {
        let defaults = DetectionConfig {
            calendar: true,
            processes: true,
            audio_activity: true,
            min_attendees: 2,
        };
        assert_eq!(DetectionConfig::default(), defaults);
        for raw in [
            "",
            "// only a comment\n",
            "{}",
            r#"{ "detection": {} }"#,
            r#"{ "calendar": { "refresh_minutes": 5 } }"#,
        ] {
            assert_eq!(parse_detection(raw).unwrap(), defaults, "{raw:?}");
        }
    }

    #[test]
    fn each_switch_turns_off_with_a_config_edit_alone() {
        let off = |key: &str| {
            parse_detection(&format!(r#"{{ "detection": {{ "{key}": false }} }}"#)).unwrap()
        };
        let on = DetectionConfig::default();
        assert_eq!(
            off("calendar"),
            DetectionConfig {
                calendar: false,
                ..on
            }
        );
        assert_eq!(
            off("processes"),
            DetectionConfig {
                processes: false,
                ..on
            }
        );
        assert_eq!(
            off("audio_activity"),
            DetectionConfig {
                audio_activity: false,
                ..on
            }
        );
    }

    #[test]
    fn every_key_overrides() {
        let detection = parse_detection(
            r#"{
                // JSONC: comments allowed
                "detection": {
                    "calendar": false,
                    "processes": false,
                    "audio_activity": false,
                    "min_attendees": 0
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            detection,
            DetectionConfig {
                calendar: false,
                processes: false,
                audio_activity: false,
                min_attendees: 0,
            }
        );
    }

    #[test]
    fn the_app_reader_logs_a_bad_section_and_runs_with_defaults() {
        for raw in [
            r#"{ "detection": { "processes": "no" } }"#,
            r#"{ "detection": { "min_attendees": -1 } }"#,
            r#"{ "detection": { "min_attendees": 2.5 } }"#,
            r#"{ "detection": null }"#,
            "{ not json",
        ] {
            assert!(parse_detection(raw).is_err(), "{raw:?}");
            assert_eq!(
                detection_or_defaults(raw),
                DetectionConfig::default(),
                "{raw:?}"
            );
        }
    }
}
