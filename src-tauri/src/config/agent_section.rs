//! The `agent` and `tickets` sections of `config.jsonc` (SPEC §3.5 as amended
//! by A11): which agent CLI the app runs after a call, with which model, and
//! which tracker the Sync prompt names.
//!
//! Unlike `transcription`, a bad value here is **returned**, not logged and
//! replaced by the default. The rule is A4's for `transcription.engine`: if you
//! asked for `"harness": "codx"`, you want the Setup screen to say so, not to
//! find your notes quietly ran through Claude Code. A *missing* section or key
//! is still the default.

use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

use super::read_section;
use crate::error::UiError;

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
    /// The file could not be read or written.
    Io(std::io::Error),
    /// The meetings folder itself could not be found. Kept as the original
    /// `UiError` so its own kind (say `no-home-dir`) reaches the UI.
    Root(UiError),
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
            Self::Root(error) => write!(f, "config.jsonc: {}", error.message),
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

/// One section, decoded on its own: a typo in `tickets` never fails reading
/// `agent`, or the other way round.
fn section<T: serde::de::DeserializeOwned + Default>(
    raw: &str,
    name: &str,
) -> Result<T, ConfigError> {
    read_section(raw, name)
        .map(Option::unwrap_or_default)
        .map_err(ConfigError::Invalid)
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
        model: agent.model.unwrap_or(defaults.model),
        // `""` is "not set", like `null`: the schema asks for at least one
        // character, and an empty path would only fail later, at spawn time.
        binary_path: agent
            .binary_path
            .filter(|path| !path.as_os_str().is_empty()),
        auto_run: agent.auto_run.unwrap_or(defaults.auto_run),
        timeout_sec: timeout_sec(agent.timeout_sec)?,
    })
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
