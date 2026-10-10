//! The `hooks` section of `config.jsonc` (TUR-63, SPEC §5 Phase 6): the
//! user's own commands for three moments of a meeting, and how long each may
//! run.
//!
//! Like `detection`, the app-facing reader [`hooks`] logs a bad section and
//! uses the defaults (no hooks), so a typo never stops a meeting.

use serde::Deserialize;

use super::agent_section::ConfigError;
use super::read_section;

/// How long a hook may run before its whole process tree is killed.
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 30;

/// The longest a hook may run, the same ceiling `agent.timeout_sec` has
/// (`u32::MAX` seconds, about 136 years: "never", for anyone who means it).
/// A larger value is cut to this rather than overflowing a clock (TUR-167).
pub const MAX_HOOK_TIMEOUT_SECS: u64 = u32::MAX as u64;

/// `hooks` in `config.jsonc`. A missing or blank command is no hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksConfig {
    pub on_transcript_ready: Option<String>,
    pub on_analysis_complete: Option<String>,
    pub on_meeting_end: Option<String>,
    pub timeout_secs: u64,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            on_transcript_ready: None,
            on_analysis_complete: None,
            on_meeting_end: None,
            timeout_secs: DEFAULT_HOOK_TIMEOUT_SECS,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawHooks {
    on_transcript_ready: Option<String>,
    on_analysis_complete: Option<String>,
    on_meeting_end: Option<String>,
    timeout_secs: Option<u64>,
}

/// A command worth running: set and not only whitespace.
fn command(raw: Option<String>) -> Option<String> {
    raw.map(|command| command.trim().to_owned())
        .filter(|command| !command.is_empty())
}

/// `hooks` from the text of `config.jsonc`. Empty text, or no `hooks` key,
/// is no hooks. A `timeout_secs` of 0 is the default; one over
/// [`MAX_HOOK_TIMEOUT_SECS`] is that.
pub fn parse_hooks(raw: &str) -> Result<HooksConfig, ConfigError> {
    let hooks: RawHooks = read_section(raw, "hooks")
        .map_err(ConfigError::Invalid)?
        .unwrap_or_default();
    Ok(HooksConfig {
        on_transcript_ready: command(hooks.on_transcript_ready),
        on_analysis_complete: command(hooks.on_analysis_complete),
        on_meeting_end: command(hooks.on_meeting_end),
        timeout_secs: hooks
            .timeout_secs
            .filter(|secs| *secs > 0)
            .map_or(DEFAULT_HOOK_TIMEOUT_SECS, |secs| {
                secs.min(MAX_HOOK_TIMEOUT_SECS)
            }),
    })
}

/// `hooks` from `~/Meetings/.app/config.jsonc`, or no hooks if the file or
/// section is missing or not valid (logged). Read each time a hook is due, so
/// an edit needs no restart.
pub fn hooks() -> HooksConfig {
    parse_hooks(&super::raw_or_empty()).unwrap_or_else(|error| {
        tracing::warn!(%error, "config.jsonc's hooks section is not valid; running no hooks");
        HooksConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_section_is_no_hooks_and_the_default_timeout() {
        assert_eq!(parse_hooks("").ok(), Some(HooksConfig::default()));
        assert_eq!(HooksConfig::default().timeout_secs, 30);
    }

    #[test]
    fn all_three_hooks_and_the_timeout_are_read() {
        let hooks = parse_hooks(
            r#"{ "hooks": {
                "on_transcript_ready": "~/bin/a.sh",
                "on_analysis_complete": "b",
                "on_meeting_end": "c.ps1", // comments are fine
                "timeout_secs": 5
            } }"#,
        );
        assert_eq!(
            hooks.ok(),
            Some(HooksConfig {
                on_transcript_ready: Some("~/bin/a.sh".into()),
                on_analysis_complete: Some("b".into()),
                on_meeting_end: Some("c.ps1".into()),
                timeout_secs: 5,
            })
        );
    }

    #[test]
    fn a_blank_command_is_no_hook_and_zero_is_the_default_timeout() {
        let hooks = parse_hooks(r#"{ "hooks": { "on_meeting_end": "  ", "timeout_secs": 0 } }"#);
        assert_eq!(hooks.ok(), Some(HooksConfig::default()));
    }

    #[test]
    fn a_huge_timeout_is_cut_to_the_agent_ceiling() {
        let hooks = parse_hooks(r#"{ "hooks": { "timeout_secs": 99999999999 } }"#).ok();
        assert_eq!(hooks.map(|h| h.timeout_secs), Some(MAX_HOOK_TIMEOUT_SECS));
    }

    #[test]
    fn a_wrong_type_is_an_error_for_the_reader_to_log() {
        assert!(parse_hooks(r#"{ "hooks": { "on_meeting_end": 3 } }"#).is_err());
    }
}
