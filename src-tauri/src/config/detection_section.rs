//! The `detection` section of `config.jsonc` (SPEC §3.5): which signals the
//! app uses to notice that a meeting has started.
//!
//! Each of the three switches turns one signal off with a config edit only.
//! Like every Settings section (TUR-155, `keyed.rs`), a bad value costs only
//! its own key: it is logged and read as that key's default, the good keys
//! are kept, and [`detection_checked`] returns the problem for the
//! Notifications card. `min_attendees` outside [`MIN_ATTENDEES`] to
//! [`MAX_ATTENDEES`] is clamped to the nearest end.
//!
//! The detection loops (`crate::detection`) re-read the section while they
//! run (TUR-78), so a change from Settings → Notifications or by hand applies
//! without a restart. `remind_before_minutes` (TUR-78) is how long before a
//! meeting its reminder fires: a value that is not a whole number from 0 to
//! [`MAX_REMIND_BEFORE_MINUTES`] is logged and read as the default, without
//! losing the other keys. [`update_detection`] writes the keys that changed
//! back through the comment-keeping writer in `file.rs`, and leaves the rest
//! as the user wrote them.
//!
//! TUR-143 added `call_start`, "Ask to record when a call starts", and the
//! "Never detect" list `never_detect` (its own reader, `detection_never.rs`,
//! so this section stays `Copy`).

use jsonc_parser::cst::CstInputValue;

use super::agent_section::ConfigError;
use super::file::{read_in, with_section, write_in};
use super::keyed::{Checked, Keys};

/// `detection.remind_before_minutes` when it is missing or not valid: the
/// reminder fires a minute before the start (TUR-30's behaviour).
pub const DEFAULT_REMIND_BEFORE_MINUTES: u32 = 1;

/// The longest lead time `detection.remind_before_minutes` takes.
pub const MAX_REMIND_BEFORE_MINUTES: u32 = 15;

/// The fewest attendees `detection.min_attendees` takes: the one range the
/// schema, this reader and the Notifications card share (TUR-155).
pub const MIN_ATTENDEES: u32 = 1;

/// The most attendees `detection.min_attendees` takes.
pub const MAX_ATTENDEES: u32 = 10;

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

/// `detection` from the text of `config.jsonc`, key by key, with what was
/// not valid. Empty text, or no `detection` key, is all defaults.
pub fn read_detection(raw: &str) -> Checked<DetectionConfig> {
    let mut keys = Keys::read(raw, "detection");
    let defaults = DetectionConfig::default();
    let detection = DetectionConfig {
        calendar: keys.get("calendar").unwrap_or(defaults.calendar),
        processes: keys.get("processes").unwrap_or(defaults.processes),
        audio_activity: keys
            .get("audio_activity")
            .unwrap_or(defaults.audio_activity),
        min_attendees: min_attendees(&mut keys),
        remind_before_minutes: remind_before_minutes(&mut keys),
        call_start: keys.get("call_start").unwrap_or(defaults.call_start),
        call_end: keys.get("call_end").unwrap_or(defaults.call_end),
        stop_after_silence: keys
            .get("stop_after_silence")
            .unwrap_or(defaults.stop_after_silence),
    };
    keys.checked(detection)
}

/// `detection` from the text of `config.jsonc`, each bad key logged and read
/// as its default.
pub fn parse_detection(raw: &str) -> DetectionConfig {
    read_detection(raw).value
}

/// `min_attendees` as written: a whole number, clamped to [`MIN_ATTENDEES`]
/// to [`MAX_ATTENDEES`] (logged), or (logged) the default.
fn min_attendees(keys: &mut Keys) -> u32 {
    const KEY: &str = "min_attendees";
    let Some(value) = keys.raw(KEY).cloned() else {
        return DetectionConfig::default().min_attendees;
    };
    let whole = value
        .as_i64()
        .or_else(|| value.as_u64().map(|n| i64::try_from(n).unwrap_or(i64::MAX)));
    match whole {
        Some(count) => {
            let clamped = count.clamp(i64::from(MIN_ATTENDEES), i64::from(MAX_ATTENDEES));
            if clamped != count {
                keys.bad(
                    KEY,
                    format!(
                        "{count} is outside {MIN_ATTENDEES} to {MAX_ATTENDEES}; using {clamped}"
                    ),
                );
            }
            // Clamped into 1..=10 above, so it fits.
            u32::try_from(clamped).unwrap_or(MAX_ATTENDEES)
        }
        None => {
            let default = DetectionConfig::default().min_attendees;
            keys.bad(
                KEY,
                format!("{value} is not a whole number; using {default}"),
            );
            default
        }
    }
}

/// `remind_before_minutes` as written: a whole number from 0 to
/// [`MAX_REMIND_BEFORE_MINUTES`], or (logged) the default.
fn remind_before_minutes(keys: &mut Keys) -> u32 {
    const KEY: &str = "remind_before_minutes";
    let Some(value) = keys.raw(KEY).cloned() else {
        return DEFAULT_REMIND_BEFORE_MINUTES;
    };
    match value
        .as_u64()
        .and_then(|minutes| u32::try_from(minutes).ok())
    {
        Some(minutes) if minutes <= MAX_REMIND_BEFORE_MINUTES => minutes,
        _ => {
            keys.bad(
                KEY,
                format!(
                    "{value} is not a whole number from 0 to {MAX_REMIND_BEFORE_MINUTES}; using {DEFAULT_REMIND_BEFORE_MINUTES}"
                ),
            );
            DEFAULT_REMIND_BEFORE_MINUTES
        }
    }
}

/// `detection` from `~/Meetings/.app/config.jsonc`, each bad key read as its
/// SPEC §3.5 default (logged).
pub fn detection() -> DetectionConfig {
    parse_detection(&super::raw_or_empty())
}

/// [`detection`], with what was not valid, for the Notifications card.
pub fn detection_checked() -> Checked<DetectionConfig> {
    read_detection(&super::raw_or_empty())
}

/// `raw` with its `detection` section set to `detection`, comments and other
/// keys kept.
#[cfg(test)]
pub fn with_detection(raw: &str, detection: &DetectionConfig) -> Result<String, ConfigError> {
    with_section(raw, "detection", fields(detection, None))
}

/// The keys of `detection` to write: all of them, or only those that differ
/// from `before`.
fn fields(
    detection: &DetectionConfig,
    before: Option<&DetectionConfig>,
) -> Vec<(&'static str, CstInputValue)> {
    let mut fields = Vec::new();
    macro_rules! key {
        ($field:ident) => {
            if before.is_none_or(|old| old.$field != detection.$field) {
                fields.push((stringify!($field), detection.$field.into()));
            }
        };
    }
    key!(calendar);
    key!(processes);
    key!(audio_activity);
    key!(min_attendees);
    key!(remind_before_minutes);
    key!(call_start);
    key!(call_end);
    key!(stop_after_silence);
    fields
}

/// Save the `detection` section `merge` makes from the one on disk (each bad
/// key read as its default), and return it as read back. Only the keys that
/// differ from the file are written, so a bad key `merge` left alone stays
/// as the user wrote it. `merge` runs under the write lock and may refuse.
pub fn update_detection<E: From<ConfigError>>(
    merge: impl FnOnce(&DetectionConfig) -> Result<DetectionConfig, E>,
) -> Result<DetectionConfig, E> {
    let dir = super::app_dir().map_err(ConfigError::Root)?;
    update_detection_in(&dir, merge)
}

/// [`update_detection`] for the config folder `dir`.
fn update_detection_in<E: From<ConfigError>>(
    dir: &std::path::Path,
    merge: impl FnOnce(&DetectionConfig) -> Result<DetectionConfig, E>,
) -> Result<DetectionConfig, E> {
    write_in(dir, |raw| -> Result<String, E> {
        let before = parse_detection(raw);
        let wanted = merge(&before)?;
        Ok(with_section(
            raw,
            "detection",
            fields(&wanted, Some(&before)),
        )?)
    })?;
    Ok(parse_detection(&read_in(dir).map_err(E::from)?))
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
            assert_eq!(
                read_detection(raw),
                Checked {
                    value: defaults,
                    problem: None
                },
                "{raw:?}"
            );
        }
    }

    #[test]
    fn each_switch_turns_off_with_a_config_edit_alone() {
        let off =
            |key: &str| parse_detection(&format!(r#"{{ "detection": {{ "{key}": false }} }}"#));
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
                    "min_attendees": 7,
                    "remind_before_minutes": 10,
                    "call_start": false,
                    "call_end": false,
                    "stop_after_silence": false
                }
            }"#,
        );
        assert_eq!(
            detection,
            DetectionConfig {
                calendar: false,
                processes: false,
                audio_activity: false,
                min_attendees: 7,
                remind_before_minutes: 10,
                call_start: false,
                call_end: false,
                stop_after_silence: false,
            }
        );
    }

    #[test]
    fn a_bad_value_is_a_problem_and_reads_as_its_default_keeping_the_rest() {
        for raw in [
            r#"{ "detection": { "processes": "no" } }"#,
            r#"{ "detection": { "min_attendees": 2.5 } }"#,
            r#"{ "detection": { "min_attendees": "4" } }"#,
            r#"{ "detection": null }"#,
            "{ not json",
        ] {
            let read = read_detection(raw);
            assert!(read.problem.is_some(), "{raw:?}");
            assert_eq!(read.value, DetectionConfig::default(), "{raw:?}");
        }
        let read = read_detection(r#"{ "detection": { "processes": "no", "calendar": false } }"#);
        assert!(!read.value.calendar, "the good key is kept");
        assert!(read.value.processes);
    }

    #[test]
    fn min_attendees_out_of_range_is_clamped_with_a_problem() {
        for (written, read) in [
            ("0", MIN_ATTENDEES),
            ("-1", MIN_ATTENDEES),
            ("12", MAX_ATTENDEES),
            ("20", MAX_ATTENDEES),
            ("9999999999", MAX_ATTENDEES),
        ] {
            let raw = format!(
                r#"{{ "detection": {{ "processes": false, "min_attendees": {written} }} }}"#
            );
            let checked = read_detection(&raw);
            assert_eq!(checked.value.min_attendees, read, "{written}");
            assert!(
                !checked.value.processes,
                "the other keys survive: {written}"
            );
            let problem = checked.problem.unwrap();
            assert!(problem.contains("outside 1 to 10"), "{problem}");
        }
        for count in MIN_ATTENDEES..=MAX_ATTENDEES {
            let raw = format!(r#"{{ "detection": {{ "min_attendees": {count} }} }}"#);
            assert_eq!(read_detection(&raw).problem, None, "{count}");
            assert_eq!(parse_detection(&raw).min_attendees, count);
        }
    }

    #[test]
    fn a_save_beside_a_bad_min_attendees_writes_only_the_change() {
        // TUR-155: `"min_attendees": 20` used to make every toggle on the
        // card fail. Now the toggle lands and the hand-written 20 stays.
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        std::fs::create_dir_all(&dir).unwrap();
        let raw = "{\n  // mine\n  \"detection\": { \"min_attendees\": 20 }\n}\n";
        std::fs::write(dir.join(super::super::FILE), raw).unwrap();

        let saved = update_detection_in::<ConfigError>(&dir, |on_disk| {
            assert_eq!(on_disk.min_attendees, MAX_ATTENDEES);
            Ok(DetectionConfig {
                processes: false,
                ..*on_disk
            })
        })
        .unwrap();
        assert!(!saved.processes);
        assert_eq!(saved.min_attendees, MAX_ATTENDEES);
        let written = read_in(&dir).unwrap();
        assert!(written.contains(r#""min_attendees": 20"#), "{written}");
        assert!(written.contains(r#""processes": false"#), "{written}");
        assert!(written.contains("// mine"), "{written}");
        assert!(
            !written.contains("calendar"),
            "untouched keys are not written: {written}"
        );
    }

    #[test]
    fn a_refusing_merge_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("app-dir");
        let error = update_detection_in(&dir, |_| Err(ConfigError::Invalid("no".into())));
        assert!(error.is_err());
        assert_eq!(read_in(&dir).unwrap(), "");
    }

    #[test]
    fn the_lead_time_reads_every_allowed_value() {
        for minutes in [0, 1, 2, 5, 10, 15] {
            let raw = format!(r#"{{ "detection": {{ "remind_before_minutes": {minutes} }} }}"#);
            assert_eq!(parse_detection(&raw).remind_before_minutes, minutes);
        }
    }

    #[test]
    fn a_bad_lead_time_is_logged_and_read_as_one_minute_keeping_the_rest() {
        for bad in ["16", "-1", "2.5", "\"5\"", "null", "true", "99999999999"] {
            let raw = format!(
                r#"{{ "detection": {{ "processes": false, "remind_before_minutes": {bad} }} }}"#
            );
            let checked = read_detection(&raw);
            let detection = checked.value;
            assert_eq!(detection.remind_before_minutes, 1, "{bad}");
            if bad != "null" {
                assert!(checked.problem.is_some(), "{bad}");
            }
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
        assert_eq!(parse_detection(&written), changed);
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
        assert_eq!(parse_detection(&read_in(&dir).unwrap()), changed);
    }

    #[test]
    fn the_schema_lead_time_matches_the_code() {
        let schema: serde_json::Value = serde_json::from_str(super::super::file::SCHEMA).unwrap();
        let key = &schema["properties"]["detection"]["properties"]["remind_before_minutes"];
        assert_eq!(key["type"], "integer");
        assert_eq!(key["minimum"], 0);
        assert_eq!(key["maximum"], MAX_REMIND_BEFORE_MINUTES);
        assert_eq!(key["default"], DEFAULT_REMIND_BEFORE_MINUTES);
        let key = &schema["properties"]["detection"]["properties"]["min_attendees"];
        assert_eq!(key["type"], "integer");
        assert_eq!(key["minimum"], MIN_ATTENDEES);
        assert_eq!(key["maximum"], MAX_ATTENDEES);
        assert_eq!(key["default"], DetectionConfig::default().min_attendees);
        let call_start = &schema["properties"]["detection"]["properties"]["call_start"];
        assert_eq!(call_start["type"], "boolean");
        assert_eq!(call_start["default"], DetectionConfig::default().call_start);
    }
}
