//! The `agent` and `tickets` sections of `config.jsonc` (SPEC §3.5 as amended
//! by A11): which agent CLI the app runs after a call, with which model, and
//! which tracker the Sync prompt names.
//!
//! Unlike `transcription`, a bad value here is **returned**, not logged and
//! replaced by the default. The rule is A4's for `transcription.engine`: if you
//! asked for `"harness": "codx"`, you want the Setup screen to say so, not to
//! find your notes quietly ran through Claude Code. A *missing* section or key
//! is still the default.

use std::path::PathBuf;

use serde::Deserialize;

use super::error::ConfigError;
use super::read_section;
// One section, decoded on its own: a typo in `tickets` never fails reading
// `agent`, or the other way round.
use super::section::strict as section;

/// Which agent CLI runs the notes and Sync jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Harness {
    /// Anthropic's `claude` CLI.
    #[default]
    ClaudeCode,
    /// OpenAI's `codex` CLI.
    Codex,
    /// No CLI: the app puts the prompt on the clipboard instead (A11's
    /// copy-prompt fallback).
    None,
}

impl Harness {
    /// Every value, in the order `config.schema.json` lists them.
    pub const ALL: [Self; 3] = [Self::ClaudeCode, Self::Codex, Self::None];

    /// The spelling in `config.jsonc`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::None => "none",
        }
    }

    fn from_config(name: &str) -> Result<Self, ConfigError> {
        Self::ALL
            .into_iter()
            .find(|harness| harness.as_str() == name)
            .ok_or_else(|| ConfigError::UnknownHarness(name.to_string()))
    }
}

/// `agent` in `config.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    pub harness: Harness,
    /// Any model name the chosen CLI accepts. `None` (`null` or blank in the
    /// file) passes no `--model`, so the CLI picks its own (SPEC A14).
    pub model: Option<String>,
    /// Set when auto-detect cannot find the CLI on the login shell's `PATH`.
    pub binary_path: Option<PathBuf>,
    /// Notes start on their own when a call ends.
    pub auto_run: bool,
    /// Time limit for one run, in seconds. Never 0.
    pub timeout_sec: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            harness: Harness::default(),
            model: None,
            binary_path: None,
            auto_run: true,
            timeout_sec: 300,
        }
    }
}

/// `tickets` in `config.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketsConfig {
    pub tracker: String,
    /// The MCP server name as the chosen CLI lists it. Named in the Sync
    /// prompt; the app never calls it itself (L11).
    pub tracker_mcp: String,
}

impl Default for TicketsConfig {
    fn default() -> Self {
        Self {
            tracker: "linear".to_string(),
            tracker_mcp: "claude.ai Linear".to_string(),
        }
    }
}

/// `agent` as written. Every key optional, so a missing one is the default;
/// unknown keys are ignored here and kept by the writer.
#[derive(Debug, Default, Deserialize)]
struct RawAgent {
    // A string, not `Harness`, so an unknown name becomes `UnknownHarness`
    // rather than a generic serde message.
    harness: Option<String>,
    model: Option<String>,
    binary_path: Option<PathBuf>,
    auto_run: Option<bool>,
    // A float, so `300.0` reads like the schema's `integer` allows; checked
    // to be whole below.
    timeout_sec: Option<f64>,
}

/// `tickets` as written, same rules as [`RawAgent`].
#[derive(Debug, Default, Deserialize)]
struct RawTickets {
    tracker: Option<String>,
    tracker_mcp: Option<String>,
}

fn timeout_sec(value: Option<f64>) -> Result<u64, ConfigError> {
    let Some(value) = value else {
        return Ok(AgentConfig::default().timeout_sec);
    };
    if value.fract() != 0.0 || !(1.0..=f64::from(u32::MAX)).contains(&value) {
        return Err(ConfigError::Invalid(format!(
            "agent.timeout_sec must be a whole number of seconds, at least 1 (got {value})"
        )));
    }
    // Whole and in range, checked above, so the cast loses nothing.
    Ok(value as u64)
}

/// `agent` from the text of `config.jsonc`. Empty text is all defaults.
pub fn parse_agent(raw: &str) -> Result<AgentConfig, ConfigError> {
    let agent: RawAgent = section(raw, "agent")?;
    let defaults = AgentConfig::default();
    Ok(AgentConfig {
        harness: match agent.harness {
            Some(name) => Harness::from_config(&name)?,
            None => defaults.harness,
        },
        // Blank is "not set", like `null`: the CLI's own default (A14).
        model: agent
            .model
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty())
            .or(defaults.model),
        // `""` is "not set", like `null`: the schema asks for at least one
        // character, and an empty path would only fail later, at spawn time.
        binary_path: agent
            .binary_path
            .filter(|path| !path.as_os_str().is_empty()),
        auto_run: agent.auto_run.unwrap_or(defaults.auto_run),
        timeout_sec: timeout_sec(agent.timeout_sec)?,
    })
}

/// `agent.harness` from the text of `config.jsonc`, whatever else in `agent`
/// is wrong (TUR-155): the tracker settings only show which agent is picked,
/// so a bad `timeout_sec` must not fail them. A bad or unknown `harness` is
/// logged and read as the default.
pub fn parse_harness_best_effort(raw: &str) -> Harness {
    let mut keys = super::keyed::Keys::read(raw, "agent");
    let name: Option<String> = keys.get("harness");
    match name.as_deref().map(Harness::from_config) {
        None => Harness::default(),
        Some(Ok(harness)) => harness,
        Some(Err(error)) => {
            tracing::warn!(%error, "config.jsonc's agent.harness is not valid; showing the default");
            Harness::default()
        }
    }
}

/// `tickets` from the text of `config.jsonc`. Empty text is all defaults.
pub fn parse_tickets(raw: &str) -> Result<TicketsConfig, ConfigError> {
    let tickets: RawTickets = section(raw, "tickets")?;
    let defaults = TicketsConfig::default();
    Ok(TicketsConfig {
        tracker: tickets.tracker.unwrap_or(defaults.tracker),
        tracker_mcp: tickets.tracker_mcp.unwrap_or(defaults.tracker_mcp),
    })
}

/// Whether the text of `config.jsonc` names a tracker and its MCP server
/// itself, as Settings, Tracker saves them (TUR-113). The defaults alone never
/// count: no ticket is sent to a tracker the user did not pick.
pub fn parse_tickets_chosen(raw: &str) -> Result<bool, ConfigError> {
    let tickets: Option<RawTickets> = read_section(raw, "tickets").map_err(ConfigError::Invalid)?;
    Ok(tickets.is_some_and(|tickets| {
        let named = |value: &Option<String>| value.as_deref().is_some_and(|v| !v.trim().is_empty());
        named(&tickets.tracker) && named(&tickets.tracker_mcp)
    }))
}

#[derive(Debug, Default, Deserialize)]
struct RawRepos {
    default: Option<String>,
}

/// `repos.default` from the text of `config.jsonc`: the repo Start Work names
/// when a meeting links none (TUR-8). `None` when unset or blank.
pub fn parse_default_repo(raw: &str) -> Result<Option<String>, ConfigError> {
    let repos: RawRepos = section(raw, "repos")?;
    Ok(repos.default.filter(|path| !path.trim().is_empty()))
}
