//! Which MCP servers the user's agent CLI can see, for the tracker picker in
//! Settings, and the tool names a sync run gets for the one the user picked
//! (SPEC A11, TUR-11).
//!
//! The list comes from the CLI's own `mcp list` command, not from the
//! start-up event of a run: on Claude Code 2.1.286 that event's
//! `mcp_servers` list was empty even when the connector worked.
//!
//! Each list runs like a probe ([`process::run_probe`]): a fresh, empty temp
//! folder, empty stdin, its own process group, killed at the time limit. So
//! servers that only one project sets up do not show, which is exactly what a
//! sync run sees: it also starts in a temp folder, where only the user's own
//! servers load.
//!
//! Only each server's name and status are kept. The rest of what the CLI
//! prints (URLs, commands, arguments) is dropped, and none of it goes into an
//! error.

use std::path::Path;
use std::time::Duration;

use crate::AgentError;
use crate::process::{self, cli_command};

/// Time limit for `mcp list`. Claude Code checks every server's health before
/// it prints anything, and each check can take a while.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// Codex's name as the user knows it, for errors such as "Codex is not
/// installed".
const CODEX_DISPLAY_NAME: &str = "Codex";

/// The line Claude Code prints before the list.
const CLAUDE_HEADER: &str = "Checking MCP server health";

/// What the CLI says about one server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpStatus {
    /// Claude Code reached it.
    Connected,
    /// It needs the user to sign in first.
    NeedsAuth,
    /// Claude Code could not reach it.
    Failed,
    /// Waiting for the user to approve it (a project `.mcp.json`).
    Pending,
    /// Turned off in the CLI's config.
    Disabled,
    /// Set up, but not checked: Codex does not health-check its servers.
    Configured,
    /// A status this code does not know.
    Unknown,
}

/// One MCP server the CLI lists.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct McpServer {
    /// The name as the CLI lists it (`claude.ai Linear`, `linear-server`).
    /// This is what `tickets.tracker_mcp` stores.
    pub name: String,
    pub status: McpStatus,
}

/// `claude mcp list` in a fresh temp folder, empty stdin.
///
/// Claude Code connects to every server to check it, so this can take many
/// seconds; call it off the UI thread. A non-zero exit is
/// [`AgentError::CliFailed`].
pub fn claude_servers(binary: &Path, timeout: Duration) -> Result<Vec<McpServer>, AgentError> {
    let mut command = cli_command(binary);
    command.args(["mcp", "list"]);
    let out = process::run_probe(
        crate::claude::DISPLAY_NAME,
        command,
        timeout,
        &std::env::temp_dir(),
    )?;
    Ok(parse_claude_list(&out.stdout))
}

/// `codex mcp list --json` in a fresh temp folder.
///
/// Codex reads its config and prints; it does not connect to the servers. A
/// non-zero exit is [`AgentError::CliFailed`], and output that is not the
/// expected JSON is [`AgentError::InvalidJson`].
pub fn codex_servers(binary: &Path, timeout: Duration) -> Result<Vec<McpServer>, AgentError> {
    let mut command = cli_command(binary);
    command.args(["mcp", "list", "--json"]);
    let out = process::run_probe(CODEX_DISPLAY_NAME, command, timeout, &std::env::temp_dir())?;
    parse_codex_list(&out.stdout)
}

/// Reads what `claude mcp list` prints, one server per line:
///
/// ```text
/// claude.ai Linear: https://mcp.linear.app/mcp - ✔ Connected
/// linear-server: https://mcp.linear.app/mcp (HTTP) - ! Needs authentication
/// my-tool: npx -y some-pkg --flag=a:b - ✔ Connected
/// ```
///
/// The name is the text before the first `": "`, the status the text after
/// the last `" - "`. Lines without both, and the "Checking MCP server
/// health…" header, are skipped, so "No MCP servers configured" gives an
/// empty list. A name listed twice keeps its first line. Order is kept.
pub fn parse_claude_list(stdout: &str) -> Vec<McpServer> {
    let mut servers: Vec<McpServer> = Vec::new();
    for line in stdout.lines().map(str::trim) {
        if line.starts_with(CLAUDE_HEADER) {
            continue;
        }
        let Some((name, rest)) = line.split_once(": ") else {
            continue;
        };
        let Some((_, status)) = rest.rsplit_once(" - ") else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || servers.iter().any(|s| s.name == name) {
            continue;
        }
        servers.push(McpServer {
            name: name.to_owned(),
            status: claude_status(status),
        });
    }
    servers
}

/// The status Claude Code printed, by keyword, ignoring case and the glyph
/// in front of it (`✔`, `✓`, `!`, `✗`, `⏸`).
fn claude_status(text: &str) -> McpStatus {
    let text = text.to_lowercase();
    if text.contains("needs auth") {
        McpStatus::NeedsAuth
    } else if text.contains("failed") {
        McpStatus::Failed
    } else if text.contains("pending") {
        McpStatus::Pending
    } else if text.contains("disabled") {
        McpStatus::Disabled
    } else if text.contains("connected") && !text.contains("disconnected") {
        McpStatus::Connected
    } else {
        McpStatus::Unknown
    }
}

/// One entry of `codex mcp list --json`. Other fields are ignored.
#[derive(serde::Deserialize)]
struct CodexEntry {
    name: String,
    /// Missing counts as on.
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    auth_status: Option<String>,
}

/// Reads what `codex mcp list --json` prints: an array of servers with
/// `name`, `enabled` and `auth_status`.
///
/// `enabled: false` is [`McpStatus::Disabled`], `auth_status:
/// "not_logged_in"` is [`McpStatus::NeedsAuth`], and anything else is
/// [`McpStatus::Configured`], since Codex does not check its servers. A name
/// listed twice keeps its first entry. Anything that is not that JSON is
/// [`AgentError::InvalidJson`]; the error says where it broke, never what the
/// text was.
pub fn parse_codex_list(stdout: &str) -> Result<Vec<McpServer>, AgentError> {
    let entries: Vec<CodexEntry> =
        serde_json::from_str(stdout.trim()).map_err(|e| AgentError::InvalidJson {
            reason: format!(
                "Codex's MCP server list was not the expected JSON (line {}, column {})",
                e.line(),
                e.column()
            ),
        })?;
    let mut servers: Vec<McpServer> = Vec::with_capacity(entries.len());
    for entry in entries {
        if servers.iter().any(|s| s.name == entry.name) {
            continue;
        }
        let status = if entry.enabled == Some(false) {
            McpStatus::Disabled
        } else if entry.auth_status.as_deref() == Some("not_logged_in") {
            McpStatus::NeedsAuth
        } else {
            McpStatus::Configured
        };
        servers.push(McpServer {
            name: entry.name,
            status,
        });
    }
    Ok(servers)
}

/// The `--allowedTools` (Claude Code) or `allowed_tools` (Codex) entries for a
/// sync run that may use only `server`'s tools: every tool of that server.
///
/// `harness_id` is [`crate::claude::ID`] or `"codex"`.
///
/// - Claude Code names a server's tools `mcp__<server>__<tool>` with every
///   character outside `[A-Za-z0-9_-]` turned into `_`, so `claude.ai Linear`
///   gives `mcp__claude_ai_Linear__*`.
/// - Codex, and any other harness, gets the config name as is:
///   `mcp__linear__*`. The Codex harness turns that into "all of this
///   server's tools".
///
/// Empty for an empty or blank server name.
pub fn tracker_tools(harness_id: &str, server: &str) -> Vec<String> {
    if server.trim().is_empty() {
        return Vec::new();
    }
    let name = if harness_id == crate::claude::ID {
        claude_server_key(server)
    } else {
        server.to_owned()
    };
    vec![format!("mcp__{name}__*")]
}

/// A server name as Claude Code writes it inside tool names.
fn claude_server_key(server: &str) -> String {
    server
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
