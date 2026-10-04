//! Reading `transcription.engine` and `transcription.model` from
//! `~/Meetings/.app/config.jsonc`.
//!
//! This is deliberately not a config system. Phase 6 (SPEC §5) owns the rest of
//! §3.5 — `audio`, `calendar`, `detection`, `repos`, and even
//! `transcription.language`/`transcription.live` — plus the settings UI to edit
//! it. (A11's `agent` and `tickets`, and `config.schema.json`, came early; see
//! below.) This module exists only to make the Phase 1 exit
//! gate true: **"engine switch is a config change only."** Before this,
//! `src-tauri/src/engine.rs` hardcoded the engine to
//! [`stt::registry::Preference::Auto`][crate::engine], so trying whisper meant
//! rebuilding, not editing a file.
//!
//! `meetings_root` is the one §3.5 key that jumped ahead of Phase 6 (old
//! TUR-82, 145d886):
//! it lives in `meetings.rs`, not here, and not in `config.jsonc` at all —
//! finding the root has to work *before* `config.jsonc` can be located, since
//! that file is itself under the root.
//!
//! When Phase 6 lands, delete this file and fold [`Transcription`] into
//! whatever struct replaces it — do not build a second reader beside it.
//!
//! A11 added the `agent` and `tickets` sections ([`agent_section`]) ahead of
//! Phase 6 too, because the Setup screens and the notes run need them now.
//! All three sections go through the one reader here, [`read_section`], which
//! decodes each section on its own so a typo in one never breaks another. What
//! differs is what a bad value does: `transcription` logs it and uses the
//! defaults (startup must not fail), while `agent` and `tickets` return it as
//! a [`ConfigError`] for the Setup screen to show. They also add the one thing
//! `transcription` never needed: writing back ([`file`]), with the user's
//! comments and unknown keys kept.
//!
//! TUR-25 added the `calendar` and `detection` readers ([`calendar_section`],
//! [`detection_section`]) ahead of Phase 6 for the calendar refresh and
//! detection loops. They behave like `transcription`: a bad value is logged
//! and the defaults are used, so startup never fails on them.
//!
//! TUR-45 added the `audio` reader ([`audio_section`]) for the retention job,
//! with the same rule.
//! TUR-85 changed that one: a file that cannot be read or parsed, or a bad
//! `audio` section, pauses the retention job rather than using the default.

use std::path::PathBuf;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use stt::registry::Preference;

use crate::error::UiError;

mod agent_section;
#[cfg(test)]
mod agent_tests;
mod audio_section;
// TUR-91: `audio.use_builtin_mic_with_bluetooth`.
mod audio_mic;
mod calendar_section;
mod detection_section;
mod file;
// TUR-63: `hooks`, the user's own commands.
mod hooks_section;
// TUR-76: `app.show_in_dock_when_closed`.
mod app_section;
#[cfg(test)]
mod transcription_tests;

pub use agent_section::ConfigError;
pub use app_section::{AppConfig, app, set_app};
pub use audio_mic::{set_use_builtin_mic_with_bluetooth, use_builtin_mic_with_bluetooth};
pub use audio_section::Policy as RetentionPolicy;
pub use audio_section::audio;
#[cfg(test)]
pub(crate) use audio_section::policy_at as retention_policy_at;
// TUR-28 (calendar refresh loop) uses these.
#[allow(unused_imports)]
pub use calendar_section::{CalendarConfig, Provider, calendar, parse_calendar};
pub use detection_section::detection;
pub use hooks_section::{HooksConfig, hooks};
// TUR-49: the Settings card connects and disconnects calendar sources.
pub use calendar_section::set_providers as set_calendar_providers;
// TUR-78: Settings → Notifications writes the section; the reminder reads the lead time.
pub use detection_section::{DEFAULT_REMIND_BEFORE_MINUTES, DetectionConfig, set_detection};
// TUR-9 (Setup screens) adds the IPC commands that use these.
#[allow(unused_imports)]
pub use agent_section::{AgentConfig, Harness, TicketsConfig};
pub use file::default_repo;
pub use file::set_transcription;
#[allow(unused_imports)] // TUR-9, same
pub use file::{agent, set_agent, set_tickets, tickets};
// TUR-90: the Setup screen's save merges under the config write lock.
pub use file::update_agent;
// TUR-101: the agent setup tests write and read `agent` back as text.
#[cfg(test)]
pub(crate) use {agent_section::parse_agent, file::with_agent};

const FILE: &str = "config.jsonc";

/// The two `transcription` keys this phase reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Transcription {
    #[serde(default)]
    pub engine: Preference,
    #[serde(default = "default_model")]
    pub model: String,
}

impl Default for Transcription {
    fn default() -> Self {
        Self {
            engine: Preference::default(),
            model: default_model(),
        }
    }
}

fn default_model() -> String {
    // TUR-61: by hardware tier, not one fixed model.
    crate::engine::default_model()
}

/// `~/Meetings/.app`, the folder `config.jsonc` lives in.
fn app_dir() -> Result<PathBuf, UiError> {
    crate::meetings::root().map(|root| meeting_format::layout::app_dir(&root))
}

fn path() -> Option<PathBuf> {
    app_dir().ok().map(|dir| dir.join(FILE))
}

/// The text of `config.jsonc`, or `""` (all defaults) when the meetings
/// folder cannot be found or the file cannot be read. For the sections that,
/// like `transcription`, must never stop the app from starting.
fn raw_or_empty() -> String {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}

/// The one JSONC reader for `config.jsonc`: the top-level section `name`,
/// decoded on its own. `Ok(None)` when the file is empty, is not an object,
/// or has no such key. `Err` is the parse or decode message, ready to show.
fn read_section<T: DeserializeOwned>(raw: &str, name: &str) -> Result<Option<T>, String> {
    let file =
        jsonc_parser::parse_to_serde_value::<Option<serde_json::Value>>(raw, &Default::default())
            .map_err(|error| error.to_string())?;
    match file.as_ref().and_then(|file| file.get(name)) {
        None => Ok(None),
        Some(section) => T::deserialize(section)
            .map(Some)
            .map_err(|error| format!("{name}: {error}")),
    }
}

/// Read `transcription.engine`/`transcription.model`, or the SPEC §3.5
/// defaults if the file is missing, unreadable, or fails to parse.
///
/// Mirrors `onboarding::state`'s rule: a config problem must never be why the
/// app fails to start. The failure this phase *does* surface to the user is
/// asking for an engine this Mac cannot provide — `stt::registry::resolve`
/// reports that, and it reaches the UI through the existing `UiError`
/// conversion (see `error.rs`), not through this function.
pub fn transcription() -> Transcription {
    let Some(path) = path() else {
        return Transcription::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Transcription::default();
    };
    parse(&raw)
}

fn parse(raw: &str) -> Transcription {
    match read_section::<Transcription>(raw, "transcription") {
        Ok(transcription) => transcription.unwrap_or_default(),
        Err(error) => {
            tracing::warn!(%error, "config.jsonc's transcription block did not parse; using defaults");
            Transcription::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_the_spec_3_5_defaults() {
        assert_eq!(parse(""), Transcription::default());
        assert_eq!(Transcription::default().engine, Preference::Auto);
        assert_eq!(
            Transcription::default().model,
            crate::engine::default_model()
        );
    }

    #[test]
    fn the_gate_this_module_exists_for_engine_round_trips_unaided() {
        // The Phase 1 exit gate, verbatim: editing this one value and
        // restarting must be enough to switch engines.
        let transcription = parse(r#"{ "transcription": { "engine": "whisper" } }"#);
        assert_eq!(transcription.engine, Preference::Whisper);

        let transcription = parse(r#"{ "transcription": { "engine": "apple-speech" } }"#);
        assert_eq!(transcription.engine, Preference::AppleSpeech);
    }

    #[test]
    fn the_model_round_trips_alongside_the_engine() {
        let transcription =
            parse(r#"{ "transcription": { "engine": "whisper", "model": "small.en-q5_1" } }"#);
        assert_eq!(transcription.engine, Preference::Whisper);
        assert_eq!(transcription.model, "small.en-q5_1");
    }

    #[test]
    fn the_spec_3_5_example_with_its_amendment_a4_engine_key_parses() {
        // SPEC §3.5, corrected by amendment A4. Comments included on purpose:
        // this is JSONC, and a plain `serde_json` reader would reject them.
        let transcription = parse(
            r#"{
                "transcription": {
                    "engine": "auto",              // or "apple-speech" | "whisper"
                    "model": "large-v3-turbo-q5_0",
                    "language": "en",
                    "live": true
                }
            }"#,
        );
        assert_eq!(transcription.engine, Preference::Auto);
        assert_eq!(transcription.model, "large-v3-turbo-q5_0");
    }

    #[test]
    fn a_missing_engine_key_defaults_to_auto_without_losing_the_model() {
        let transcription = parse(r#"{ "transcription": { "model": "small.en-q5_1" } }"#);
        assert_eq!(transcription.engine, Preference::Auto);
        assert_eq!(transcription.model, "small.en-q5_1");
    }

    #[test]
    fn corrupt_config_falls_back_to_defaults_rather_than_failing_startup() {
        assert_eq!(parse("{ not json"), Transcription::default());
    }

    #[test]
    fn an_unknown_engine_name_is_a_parse_error_not_a_silent_auto() {
        // Distinguish "the key is absent" (defaults to auto, previous test)
        // from "the key is present but wrong" (the whole block is rejected and
        // logged, so a typo in config.jsonc is visible in the log rather than
        // quietly behaving like auto).
        let transcription = parse(r#"{ "transcription": { "engine": "whispr" } }"#);
        assert_eq!(transcription, Transcription::default());
    }
}
