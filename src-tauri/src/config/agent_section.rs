//! The `agent` and `tickets` sections of `config.jsonc` (SPEC §3.5 as amended
//! by A11): which agent CLI the app runs after a call, with which model, and
//! which tracker the Sync prompt names.
//!
//! Unlike `transcription`, a bad value here is **returned**, not logged and
//! replaced by the default. The rule is A4's for `transcription.engine`: if you
//! asked for `"harness": "codx"`, you want the Setup screen to say so, not to
//! find your notes quietly ran through Claude Code. A *missing* section or key
//! is still the default.

// TUR-9 (Setup screens) adds the IPC commands that call into this module.
#![allow(dead_code)]

use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

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
    /// Any model name the chosen CLI accepts.
    pub model: String,
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
            model: "opus".to_string(),
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

/// Why the `agent` or `tickets` section could not be read or written.
#[derive(Debug)]
pub enum ConfigError {
    /// `agent.harness` names a CLI the app does not know.
    UnknownHarness(String),
    /// The file is not valid JSONC, or a key has the wrong type or range.
    Invalid(String),
    /// The file or the meetings folder could not be read or written.
    Io(std::io::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHarness(name) => {
                let known: Vec<_> = Harness::ALL.iter().map(|h| h.as_str()).collect();
                write!(
                    f,
                    "config.jsonc: agent.harness {name:?} is not one of {}",
                    known.join(", ")
                )
            }
            Self::Invalid(detail) => write!(f, "config.jsonc: {detail}"),
            Self::Io(error) => write!(f, "config.jsonc: {error}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The file as written. Every key optional, so a missing one is the default;
/// unknown keys are ignored here and kept by the writer.
#[derive(Debug, Default, Deserialize)]
struct RawFile {
    #[serde(default)]
    agent: RawAgent,
    #[serde(default)]
    tickets: RawTickets,
}

#[derive(Debug, Default, Deserialize)]
struct RawAgent {
    // A string, not `Harness`, so an unknown name becomes `UnknownHarness`
    // rather than a generic serde message.
    harness: Option<String>,
    model: Option<String>,
    binary_path: Option<PathBuf>,
    auto_run: Option<bool>,
    timeout_sec: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTickets {
    tracker: Option<String>,
    tracker_mcp: Option<String>,
}

fn parse_raw(raw: &str) -> Result<RawFile, ConfigError> {
    jsonc_parser::parse_to_serde_value::<Option<RawFile>>(raw, &Default::default())
        .map(Option::unwrap_or_default)
        .map_err(|error| ConfigError::Invalid(error.to_string()))
}

/// `agent` from the text of `config.jsonc`. Empty text is all defaults.
pub fn parse_agent(raw: &str) -> Result<AgentConfig, ConfigError> {
    let agent = parse_raw(raw)?.agent;
    let defaults = AgentConfig::default();
    let timeout_sec = agent.timeout_sec.unwrap_or(defaults.timeout_sec);
    if timeout_sec == 0 {
        return Err(ConfigError::Invalid(
            "agent.timeout_sec must be at least 1".into(),
        ));
    }
    Ok(AgentConfig {
        harness: match agent.harness {
            Some(name) => Harness::from_config(&name)?,
            None => defaults.harness,
        },
        model: agent.model.unwrap_or(defaults.model),
        binary_path: agent.binary_path,
        auto_run: agent.auto_run.unwrap_or(defaults.auto_run),
        timeout_sec,
    })
}

/// `tickets` from the text of `config.jsonc`. Empty text is all defaults.
pub fn parse_tickets(raw: &str) -> Result<TicketsConfig, ConfigError> {
    let tickets = parse_raw(raw)?.tickets;
    let defaults = TicketsConfig::default();
    Ok(TicketsConfig {
        tracker: tickets.tracker.unwrap_or(defaults.tracker),
        tracker_mcp: tickets.tracker_mcp.unwrap_or(defaults.tracker_mcp),
    })
}
