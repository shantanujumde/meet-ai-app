//! Settings for Sync: which tracker, and which of the agent's MCP servers
//! reaches it (`tickets` in `config.jsonc`, SPEC §3.5 as amended by A11).
//!
//! The server list comes from `claude mcp list` / `codex mcp list --json`,
//! run in an empty temp folder like a Sync run. Not from the CLI's start-up
//! event: its `mcp_servers` list was empty on 2026-10-01 even when the
//! connector worked. A server added only for one project does not load
//! there, which is why the list shows only what a Sync run will see.

use agent::mcp::{self, McpServer, McpStatus};
use serde::Serialize;
use tauri::{AppHandle, Manager as _};

use super::{agent_error, blocking, find_binary};
use crate::config::{self, Harness as HarnessChoice, TicketsConfig};
use crate::error::UiError;
use crate::folder_move::FolderGate;

/// Trackers the Sync prompt knows how to name.
pub const TRACKERS: [&str; 3] = ["linear", "jira", "github"];

/// Longest MCP server name accepted from the window.
const MAX_SERVER_NAME: usize = 200;

/// The tracker settings as the window sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackerSettings {
    /// `linear`, `jira` or `github`.
    pub tracker: String,
    /// The MCP server name as the agent's CLI lists it.
    pub tracker_mcp: String,
    /// `agent.harness`: `claude-code`, `codex` or `none`.
    pub harness: String,
}

/// One MCP server the agent's CLI lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackerServer {
    pub name: String,
    pub status: ServerStatus,
}

/// What the CLI says about a server. Claude Code checks each one; Codex only
/// reports it as set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Connected,
    NeedsAuth,
    Failed,
    Pending,
    Disabled,
    Configured,
    Unknown,
}

impl From<McpStatus> for ServerStatus {
    fn from(status: McpStatus) -> Self {
        match status {
            McpStatus::Connected => Self::Connected,
            McpStatus::NeedsAuth => Self::NeedsAuth,
            McpStatus::Failed => Self::Failed,
            McpStatus::Pending => Self::Pending,
            McpStatus::Disabled => Self::Disabled,
            McpStatus::Configured => Self::Configured,
            McpStatus::Unknown => Self::Unknown,
        }
    }
}

impl From<McpServer> for TrackerServer {
    fn from(server: McpServer) -> Self {
        Self {
            name: server.name,
            status: server.status.into(),
        }
    }
}

/// The current tracker settings.
#[tauri::command]
#[specta::specta]
pub async fn tracker_settings() -> Result<TrackerSettings, UiError> {
    blocking(current).await?
}

/// Save the tracker and its MCP server. Through the [`FolderGate`]: the
/// config file lives in the meetings folder.
#[tauri::command]
#[specta::specta]
pub async fn set_tracker(
    app: AppHandle,
    tracker: String,
    tracker_mcp: String,
) -> Result<TrackerSettings, UiError> {
    blocking(move || {
        let tickets = checked(&tracker, &tracker_mcp)?;
        app.state::<FolderGate>()
            .writing(|| Ok(config::set_tickets(&tickets)?))?;
        current()
    })
    .await?
}

/// The MCP servers the chosen agent's CLI can see. Claude Code checks each
/// server's health, so this can take most of a minute. Empty when no agent
/// is chosen.
#[tauri::command]
#[specta::specta]
pub async fn tracker_servers() -> Result<Vec<TrackerServer>, UiError> {
    blocking(|| {
        let agent = config::agent()?;
        if agent.harness == HarnessChoice::None {
            return Ok(Vec::new());
        }
        let binary = find_binary(&agent)?;
        let servers = match agent.harness {
            HarnessChoice::Codex => mcp::codex_servers(&binary, mcp::LIST_TIMEOUT),
            _ => mcp::claude_servers(&binary, mcp::LIST_TIMEOUT),
        }
        .map_err(agent_error)?;
        Ok(servers.into_iter().map(TrackerServer::from).collect())
    })
    .await?
}

fn current() -> Result<TrackerSettings, UiError> {
    let tickets = config::tickets()?;
    Ok(TrackerSettings {
        tracker: tickets.tracker,
        tracker_mcp: tickets.tracker_mcp,
        harness: config::agent()?.harness.as_str().to_owned(),
    })
}

/// The window's values, checked: a known tracker and a plain server name.
pub(crate) fn checked(tracker: &str, tracker_mcp: &str) -> Result<TicketsConfig, UiError> {
    let tracker = tracker.trim();
    if !TRACKERS.contains(&tracker) {
        return Err(UiError::app(
            "bad-tracker",
            format!("{tracker:?} is not a tracker meet-ai knows. Pick Linear, Jira or GitHub."),
        ));
    }
    let server = tracker_mcp.trim();
    if server.is_empty() {
        return Err(UiError::app(
            "bad-tracker-server",
            "Type or pick the MCP server name the agent uses for your tracker.",
        ));
    }
    if server.chars().count() > MAX_SERVER_NAME || server.chars().any(char::is_control) {
        return Err(UiError::app(
            "bad-tracker-server",
            "That MCP server name is not one an agent CLI would list.",
        ));
    }
    Ok(TicketsConfig {
        tracker: tracker.to_owned(),
        tracker_mcp: server.to_owned(),
    })
}
