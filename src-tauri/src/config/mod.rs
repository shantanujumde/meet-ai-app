//! Reading `transcription.engine` and `transcription.model` from
//! `~/Meetings/.app/config.jsonc`.
//!
//! This is deliberately not a config system. Phase 6 (SPEC §5) owns the rest of
//! §3.5 — `audio`, `calendar`, `detection`, `tickets`, `repos`, and even
//! `transcription.language`/`transcription.live` — plus the JSON schema and the
//! settings UI to edit it. This module exists only to make the Phase 1 exit
//! gate true: **"engine switch is a config change only."** Before this,
//! `src-tauri/src/engine.rs` hardcoded the engine to
//! [`stt::registry::Preference::Auto`][crate::engine], so trying whisper meant
//! rebuilding, not editing a file.
//!
//! `meetings_root` is the one §3.5 key that jumped ahead of Phase 6 (TUR-82):
//! it lives in `meetings.rs`, not here, and not in `config.jsonc` at all —
//! finding the root has to work *before* `config.jsonc` can be located, since
//! that file is itself under the root.
//!
//! When Phase 6 lands, delete this file and fold [`Transcription`] into
//! whatever struct replaces it — do not build a second reader beside it.
//!
//! A11 added the `agent` and `tickets` sections ([`agent_section`]) ahead of
//! Phase 6 too, because the Setup screens and the notes run need them now.
//! They share this file's path and its JSONC reader, and add the one thing
//! `transcription` never needed: writing back ([`file`]), with the user's
//! comments and unknown keys kept.

use std::path::PathBuf;

use serde::Deserialize;
use stt::registry::Preference;

use crate::engine::DEFAULT_MODEL;

mod agent_section;
#[cfg(test)]
mod agent_tests;
mod file;

// TUR-9 (Setup screens) adds the IPC commands that call these.
#[allow(unused_imports)]
pub use agent_section::{AgentConfig, ConfigError, Harness, TicketsConfig};
#[allow(unused_imports)]
pub use file::{agent, set_agent, set_tickets, tickets};

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
    DEFAULT_MODEL.to_string()
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    transcription: Transcription,
}

fn path() -> Option<PathBuf> {
    crate::meetings::root()
        .ok()
        .map(|root| meeting_format::layout::app_dir(&root).join(FILE))
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
    match jsonc_parser::parse_to_serde_value::<Config>(raw, &Default::default()) {
        Ok(config) => config.transcription,
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
        assert_eq!(Transcription::default().model, DEFAULT_MODEL);
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
