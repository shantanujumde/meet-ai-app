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

use super::{agent_error, find_binary};
use crate::config::{self, Harness as HarnessChoice, TicketsConfig};
use crate::error::{UiError, on_blocking_pool};
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
    /// Whether `config.jsonc` names the tracker and server itself. False when
    /// `tracker` and `tracker_mcp` are only the shipped defaults: then no
    /// ticket is sent, and the window must not treat them as saved.
    pub chosen: bool,
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
    on_blocking_pool(current).await?
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
    on_blocking_pool(move || {
        let tickets = checked(&tracker, &tracker_mcp)?;
        app.state::<FolderGate>()
            .writing(|| Ok(config::set_tickets(&tickets)?))?;
        // TUR-113: every ticket not in the tracker yet gets one more try.
        super::auto::settings_changed(&app);
        // TUR-155: the save landed, so what goes back is what was saved, with
        // the agent read best effort. A bad `agent` section must not turn a
        // landed save into a failure the user retries.
        Ok(saved_settings(tickets, config::harness_best_effort()))
    })
    .await?
}

/// The MCP servers the chosen agent's CLI can see. Claude Code checks each
/// server's health, so this can take most of a minute. Empty when no agent
/// is chosen.
#[tauri::command]
#[specta::specta]
pub async fn tracker_servers() -> Result<Vec<TrackerServer>, UiError> {
    on_blocking_pool(|| {
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

/// One read of `config.jsonc`; a bad `agent` section shows the default
/// agent rather than failing (TUR-155).
pub(crate) fn current() -> Result<TrackerSettings, UiError> {
    let (tickets, chosen, harness) = config::tracker_view()?;
    Ok(settings_from(tickets, chosen, harness))
}

/// What `set_tracker` hands back: the tickets it saved, which the user chose.
fn saved_settings(tickets: TicketsConfig, harness: HarnessChoice) -> TrackerSettings {
    settings_from(tickets, true, harness)
}

/// The window's view of `tickets`, `chosen` and the agent.
fn settings_from(tickets: TicketsConfig, chosen: bool, harness: HarnessChoice) -> TrackerSettings {
    TrackerSettings {
        tracker: tickets.tracker,
        tracker_mcp: tickets.tracker_mcp,
        harness: harness.as_str().to_owned(),
        chosen,
    }
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
            "Type or pick the server (tracker connection, also called an MCP server) the agent uses for your tracker.",
        ));
    }
    if server.chars().count() > MAX_SERVER_NAME || server.chars().any(char::is_control) {
        return Err(UiError::app(
            "bad-tracker-server",
            "That is not a server name Claude Code or Codex would list.",
        ));
    }
    Ok(TicketsConfig {
        tracker: tracker.to_owned(),
        tracker_mcp: server.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse_tickets, parse_tickets_chosen, with_tickets};

    fn settings_in(raw: &str) -> TrackerSettings {
        settings_from(
            parse_tickets(raw).unwrap(),
            parse_tickets_chosen(raw).unwrap(),
            HarnessChoice::ClaudeCode,
        )
    }

    /// Before a save the window gets the defaults with `chosen: false`, so it
    /// never shows them as saved; once `set_tracker` wrote them, `chosen: true`.
    #[test]
    fn chosen_is_false_for_the_defaults_and_true_once_saved() {
        let defaults = settings_in("");
        assert_eq!(defaults.tracker_mcp, "claude.ai Linear");
        assert!(!defaults.chosen);
        let json = serde_json::to_value(&defaults).unwrap();
        assert_eq!(json["chosen"], serde_json::Value::Bool(false));

        let raw = with_tickets("", &checked("linear", " claude.ai Linear ").unwrap()).unwrap();
        let saved = settings_in(&raw);
        assert_eq!(saved.tracker, "linear");
        assert_eq!(saved.tracker_mcp, "claude.ai Linear");
        assert!(saved.chosen, "{raw}");
    }

    /// TUR-155: with `"timeout_sec": 0` the save used to land and then the
    /// command reported `invalid-config`. Now the saved tickets come back,
    /// and the harness beside the bad key is still read.
    #[test]
    fn a_bad_agent_section_never_fails_a_tracker_save_or_view() {
        let bad_agent = r#"{ "agent": { "harness": "codex", "timeout_sec": 0 } }"#;
        assert!(crate::config::parse_agent(bad_agent).is_err());
        let tickets = checked("jira", "Atlassian").unwrap();
        let raw = with_tickets(bad_agent, &tickets).unwrap();

        let (read, chosen, harness) = crate::config::tracker_view_of(&raw).unwrap();
        assert_eq!(read, tickets);
        assert!(chosen);
        assert_eq!(harness, HarnessChoice::Codex);

        let saved = saved_settings(tickets, harness);
        assert_eq!(saved.tracker, "jira");
        assert_eq!(saved.tracker_mcp, "Atlassian");
        assert_eq!(saved.harness, "codex");
        assert!(saved.chosen);

        let unknown = r#"{ "agent": { "harness": "codx" } }"#;
        let (_, _, harness) = crate::config::tracker_view_of(unknown).unwrap();
        assert_eq!(harness, HarnessChoice::default());
    }
}
