//! The `detection` section of `config.jsonc` (SPEC §3.5): which signals the
//! app uses to notice that a meeting has started.
//!
//! Each of the three switches turns one signal off with a config edit only.
//! Like `transcription`, a bad section is logged and the app runs with the
//! defaults ([`detection`]); [`parse_detection`] returns the error for a
//! caller that wants to show it.
//!
//! The detection loops (`crate::detection`) re-read the section while they
//! run (TUR-78), so a change from Settings → Notifications or by hand applies
//! without a restart. `remind_before_minutes` (TUR-78) is how long before a
//! meeting its reminder fires: a value that is not a whole number from 0 to
//! [`MAX_REMIND_BEFORE_MINUTES`] is logged and read as the default, without
//! losing the other keys. [`set_detection`] writes the section back through
//! the comment-keeping writer in `file.rs`.
//!
//! TUR-143 added `call_start`, "Ask to record when a call starts", and the
//! "Never detect" list `never_detect` (its own reader, `detection_never.rs`,
//! so this section stays `Copy`).

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::read_section;

/// `detection.remind_before_minutes` when it is missing or not valid: the
/// reminder fires a minute before the start (TUR-30's behaviour).
pub const DEFAULT_REMIND_BEFORE_MINUTES: u32 = 1;

/// The longest lead time `detection.remind_before_minutes` takes.
pub const MAX_REMIND_BEFORE_MINUTES: u32 = 15;

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
    /// Remind this many minutes before a meeting starts; `0` is at the start.
    pub remind_before_minutes: u32,
    /// Ask to record when a call app or a browser starts using the mic
    /// (TUR-143).
    pub call_start: bool,
    /// Ask to stop the recording when the call app hangs up (TUR-144).
    pub call_end: bool,
    /// Ask to stop a recording after 10 minutes in which no one spoke
    /// (TUR-145). Sleep always stops a recording; this has no switch.
    pub stop_after_silence: bool,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            calendar: true,
            processes: true,
            audio_activity: true,
            min_attendees: 2,
            remind_before_minutes: DEFAULT_REMIND_BEFORE_MINUTES,
            call_start: true,
            call_end: true,
            stop_after_silence: true,
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
    /// Any JSON, so a bad lead time falls back alone (see the module docs).
    remind_before_minutes: Option<serde_json::Value>,
    call_start: Option<bool>,
    call_end: Option<bool>,
    stop_after_silence: Option<bool>,
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
        remind_before_minutes: remind_before_minutes(detection.remind_before_minutes),
        call_start: detection.call_start.unwrap_or(defaults.call_start),
        call_end: detection.call_end.unwrap_or(defaults.call_end),
        stop_after_silence: detection
            .stop_after_silence
            .unwrap_or(defaults.stop_after_silence),
    })
}

/// `remind_before_minutes` as written: a whole number from 0 to
/// [`MAX_REMIND_BEFORE_MINUTES`], or (logged) the default.
fn remind_before_minutes(raw: Option<serde_json::Value>) -> u32 {
    let Some(value) = raw else {
        return DEFAULT_REMIND_BEFORE_MINUTES;
    };
    match value
        .as_u64()
        .and_then(|minutes| u32::try_from(minutes).ok())
    {
        Some(minutes) if minutes <= MAX_REMIND_BEFORE_MINUTES => minutes,
        _ => {
            tracing::warn!(
                %value,
                "detection.remind_before_minutes must be a whole number from 0 to 15; using 1"
            );
            DEFAULT_REMIND_BEFORE_MINUTES
        }
    }
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
pub fn detection() -> DetectionConfig {
    detection_or_defaults(&super::raw_or_empty())
}

/// `raw` with its `detection` section set to `detection`, comments and other
/// keys kept.
pub fn with_detection(raw: &str, detection: &DetectionConfig) -> Result<String, ConfigError> {
    with_section(
        raw,
        "detection",
        vec![
            ("calendar", detection.calendar.into()),
            ("processes", detection.processes.into()),
            ("audio_activity", detection.audio_activity.into()),
            ("min_attendees", detection.min_attendees.into()),
            (
                "remind_before_minutes",
                detection.remind_before_minutes.into(),
            ),
            ("call_start", detection.call_start.into()),
            ("call_end", detection.call_end.into()),
            ("stop_after_silence", detection.stop_after_silence.into()),
        ],
    )
}

/// Save `detection` into `~/Meetings/.app/config.jsonc`, keeping everything
/// else, and return it as read back from disk.
pub fn set_detection(detection: &DetectionConfig) -> Result<DetectionConfig, ConfigError> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    write_in(&dir, |raw| with_detection(raw, detection))?;
    parse_detection(&read_in(&dir)?)
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
            remind_before_minutes: 1,
            call_start: true,
            call_end: true,
            stop_after_silence: true,
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
        assert_eq!(
            off("call_start"),
            DetectionConfig {
                call_start: false,
                ..on
            }
        );
        assert_eq!(
            off("call_end"),
            DetectionConfig {
                call_end: false,
                ..on
            }
        );
        assert_eq!(
            off("stop_after_silence"),
            DetectionConfig {
                stop_after_silence: false,
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
                    "min_attendees": 0,
                    "remind_before_minutes": 10,
                    "call_start": false,
                    "call_end": false,
                    "stop_after_silence": false
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
                remind_before_minutes: 10,
                call_start: false,
                call_end: false,
                stop_after_silence: false,
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

    #[test]
    fn the_lead_time_reads_every_allowed_value() {
        for minutes in [0, 1, 2, 5, 10, 15] {
            let raw = format!(r#"{{ "detection": {{ "remind_before_minutes": {minutes} }} }}"#);
            assert_eq!(
                parse_detection(&raw).unwrap().remind_before_minutes,
                minutes
            );
        }
    }

    #[test]
    fn a_bad_lead_time_is_logged_and_read_as_one_minute_keeping_the_rest() {
        for bad in ["16", "-1", "2.5", "\"5\"", "null", "true", "99999999999"] {
            let raw = format!(
                r#"{{ "detection": {{ "processes": false, "remind_before_minutes": {bad} }} }}"#
            );
            let detection = parse_detection(&raw).unwrap();
            assert_eq!(detection.remind_before_minutes, 1, "{bad}");
            assert!(!detection.processes, "the other keys survive: {bad}");
        }
    }

    #[test]
    fn saving_keeps_comments_and_other_sections_and_round_trips() {
        let raw = r#"{
  // mine
  "app": { "menu_bar_countdown": true },
  "detection": { "processes": false }
}"#;
        let changed = DetectionConfig {
            calendar: false,
            audio_activity: false,
            min_attendees: 4,
            remind_before_minutes: 5,
            call_start: false,
            stop_after_silence: false,
            ..DetectionConfig::default()
        };
        let written = with_detection(raw, &changed).unwrap();
        assert!(written.contains("// mine"), "{written}");
        assert!(
            written.contains(r#""menu_bar_countdown": true"#),
            "{written}"
        );
        assert_eq!(parse_detection(&written).unwrap(), changed);
        assert_eq!(written.matches("\"processes\"").count(), 1, "{written}");
    }

    #[test]
    fn saving_to_disk_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        let changed = DetectionConfig {
            remind_before_minutes: 2,
            ..DetectionConfig::default()
        };
        write_in(&dir, |raw| with_detection(raw, &changed)).unwrap();
        assert_eq!(parse_detection(&read_in(&dir).unwrap()).unwrap(), changed);
    }

    #[test]
    fn the_schema_lead_time_matches_the_code() {
        let schema: serde_json::Value = serde_json::from_str(super::super::file::SCHEMA).unwrap();
        let key = &schema["properties"]["detection"]["properties"]["remind_before_minutes"];
        assert_eq!(key["type"], "integer");
        assert_eq!(key["minimum"], 0);
        assert_eq!(key["maximum"], MAX_REMIND_BEFORE_MINUTES);
        assert_eq!(key["default"], DEFAULT_REMIND_BEFORE_MINUTES);
        let call_start = &schema["properties"]["detection"]["properties"]["call_start"];
        assert_eq!(call_start["type"], "boolean");
        assert_eq!(call_start["default"], DetectionConfig::default().call_start);
    }
}
