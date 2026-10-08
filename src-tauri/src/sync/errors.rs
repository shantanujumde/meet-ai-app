//! What a Sync that did not create an issue says, one kind per cause, so the
//! window can show the fix next to Retry (TUR-113).
//!
//! | Cause | Kind | Says |
//! |---|---|---|
//! | The agent CLI is missing | `agent-not-installed` | install it or pick another agent |
//! | The agent CLI is signed out | `agent-not-signed-in` | the command that signs it in |
//! | The run finished with no issue and no reason: the MCP server is missing, signed out or unreachable | `sync-unreachable` | check the connection in the agent |
//! | The tracker said no (permissions, no such project) | `sync-refused` | the tracker's reason; check the project |
//! | The run timed out or was cancelled | `agent-timed-out`, `agent-cancelled` | press Retry |
//!
//! No tracker set up is not here: such a ticket is just not sent yet.

use agent::AgentError;
use prompts::push_ticket::refused_reason;
use serde_json::Value;

use crate::config::TicketsConfig;
use crate::error::UiError;

/// The run finished but the agent could not reach the tracker: no issue, and
/// no reason from the tracker. Replaces `sync-not-done`.
pub(crate) const SYNC_UNREACHABLE: &str = "sync-unreachable";

/// The tracker refused the issue and the agent passed its reason on.
pub(crate) const SYNC_REFUSED: &str = "sync-refused";

/// The one wording for a Sync run that stopped part-way.
const STOPPED: &str = "Sending stopped before it finished. Press Retry.";

/// `linear` → `Linear`, for messages.
pub(crate) fn tracker_name(tracker: &str) -> &str {
    match tracker {
        "linear" => "Linear",
        "jira" => "Jira",
        "github" => "GitHub",
        other => other,
    }
}

/// An agent run's failure, worded by `crates/agent`, with a stable kind the
/// window can switch on. For runs that are not a Sync, such as listing the
/// agent's MCP servers.
pub(crate) fn agent_error(error: AgentError) -> UiError {
    UiError::app(agent_kind(&error), error.to_string())
}

/// A Sync run's agent failure: the same kinds as [`agent_error`], worded as
/// what happened to the ticket and what to do next.
pub(crate) fn send_error(error: AgentError) -> UiError {
    let kind = agent_kind(&error);
    let message = match &error {
        AgentError::NotInstalled { harness } => format!(
            "Couldn't send: {harness} isn't installed, or meet-ai can't find it. Install it or pick another agent in Settings, Notes, then press Retry."
        ),
        AgentError::NotSignedIn { harness } => match sign_in_command(harness) {
            Some(command) => format!(
                "Couldn't send: {harness} isn't signed in. Open a terminal, run `{command}`, sign in, then press Retry."
            ),
            None => format!(
                "Couldn't send: {harness} isn't signed in. Sign in to it in a terminal, then press Retry."
            ),
        },
        AgentError::TimedOut { .. } | AgentError::Cancelled => STOPPED.to_owned(),
        _ => format!("Couldn't send: {error}."),
    };
    UiError::app(kind, message)
}

/// The error for a run that finished without creating an issue: refused when
/// the reply gives the tracker's reason, unreachable otherwise. `harness_id`
/// is the agent that ran (`agent::Harness::id`).
pub(crate) fn not_synced(
    tickets_config: &TicketsConfig,
    harness_id: &str,
    reply: &Value,
) -> UiError {
    let tracker = tracker_name(&tickets_config.tracker);
    if let Some(reason) = refused_reason(reply) {
        return UiError::app(
            SYNC_REFUSED,
            format!(
                "{tracker} refused the ticket: {reason}. Check the project in Settings, Tracker, then press Retry."
            ),
        );
    }
    UiError::app(
        SYNC_UNREACHABLE,
        format!(
            "Couldn't send to {tracker}: your agent couldn't reach {tracker}. Check that \"{}\" is connected and signed in in {}, then press Retry.",
            tickets_config.tracker_mcp,
            harness_name(harness_id)
        ),
    )
}

fn agent_kind(error: &AgentError) -> &'static str {
    match error {
        AgentError::NotInstalled { .. } => "agent-not-installed",
        AgentError::NotSignedIn { .. } => "agent-not-signed-in",
        AgentError::TimedOut { .. } => "agent-timed-out",
        AgentError::Cancelled => "agent-cancelled",
        AgentError::CliFailed { .. } => "agent-failed",
        AgentError::InvalidJson { .. } | AgentError::SchemaMismatch { .. } => "agent-bad-reply",
        AgentError::CouldNotStart { .. } => "agent-could-not-start",
    }
}

/// The agent's name for messages, by `agent::Harness::id`.
pub(super) fn harness_name(harness_id: &str) -> &'static str {
    match harness_id {
        agent::claude::ID => agent::claude::DISPLAY_NAME,
        agent::codex::ID => "Codex",
        _ => "your agent",
    }
}

/// What to type to sign `harness` (its display name, as `AgentError` gives
/// it) in, as Settings, Notes shows it (`agent_setup::view`).
fn sign_in_command(harness: &str) -> Option<&'static str> {
    match harness {
        agent::claude::DISPLAY_NAME => Some("claude auth login"),
        "Codex" => Some("codex login"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    fn linear() -> TicketsConfig {
        TicketsConfig {
            tracker: "linear".to_owned(),
            tracker_mcp: "claude.ai Linear".to_owned(),
        }
    }

    #[test]
    fn tracker_names_read_well() {
        assert_eq!(tracker_name("linear"), "Linear");
        assert_eq!(tracker_name("github"), "GitHub");
        assert_eq!(tracker_name("jira"), "Jira");
        assert_eq!(tracker_name("other"), "other");
    }

    #[test]
    fn no_issue_and_no_reason_is_unreachable_and_names_the_connection() {
        let reply = json!({ "external_id": null, "external_url": null, "refused_reason": null });
        let error = not_synced(&linear(), agent::claude::ID, &reply);
        assert_eq!(error.kind, SYNC_UNREACHABLE);
        assert_eq!(
            error.message,
            "Couldn't send to Linear: your agent couldn't reach Linear. Check that \"claude.ai Linear\" is connected and signed in in Claude Code, then press Retry."
        );
        // A reply from a template saved before refused_reason reads the same.
        let old = json!({ "external_id": null, "external_url": null });
        assert_eq!(
            not_synced(&linear(), agent::codex::ID, &old).kind,
            SYNC_UNREACHABLE
        );
        assert!(
            not_synced(&linear(), agent::codex::ID, &old)
                .message
                .contains("signed in in Codex")
        );
    }

    #[test]
    fn a_reason_from_the_tracker_is_refused_and_quoted() {
        let reply = json!({
            "external_id": null,
            "external_url": null,
            "refused_reason": "Project \"Web\" not found."
        });
        let error = not_synced(&linear(), agent::claude::ID, &reply);
        assert_eq!(error.kind, SYNC_REFUSED);
        assert_eq!(
            error.message,
            "Linear refused the ticket: Project \"Web\" not found. Check the project in Settings, Tracker, then press Retry."
        );
    }

    #[test]
    fn a_signed_out_agent_says_how_to_sign_in() {
        let error = send_error(AgentError::NotSignedIn {
            harness: agent::claude::DISPLAY_NAME.to_owned(),
        });
        assert_eq!(error.kind, "agent-not-signed-in");
        assert_eq!(
            error.message,
            "Couldn't send: Claude Code isn't signed in. Open a terminal, run `claude auth login`, sign in, then press Retry."
        );
        let codex = send_error(AgentError::NotSignedIn {
            harness: "Codex".to_owned(),
        });
        assert!(codex.message.contains("`codex login`"), "{}", codex.message);
        let other = send_error(AgentError::NotSignedIn {
            harness: "Fake".to_owned(),
        });
        assert!(
            other
                .message
                .starts_with("Couldn't send: Fake isn't signed in.")
        );
    }

    #[test]
    fn a_missing_agent_names_it() {
        let error = send_error(AgentError::NotInstalled {
            harness: "Codex".to_owned(),
        });
        assert_eq!(error.kind, "agent-not-installed");
        assert!(
            error
                .message
                .starts_with("Couldn't send: Codex isn't installed")
        );
        assert!(error.message.ends_with("then press Retry."));
    }

    #[test]
    fn a_run_that_stopped_says_press_retry() {
        for (error, kind) in [
            (
                AgentError::TimedOut {
                    after: Duration::from_secs(300),
                },
                "agent-timed-out",
            ),
            (AgentError::Cancelled, "agent-cancelled"),
        ] {
            let ui = send_error(error);
            assert_eq!(ui.kind, kind);
            assert_eq!(
                ui.message,
                "Sending stopped before it finished. Press Retry."
            );
        }
    }

    #[test]
    fn every_other_failure_keeps_its_kind_and_sentence() {
        let ui = send_error(AgentError::CliFailed {
            status: Some(2),
            stderr: "bad flag".to_owned(),
        });
        assert_eq!(ui.kind, "agent-failed");
        assert_eq!(
            ui.message,
            "Couldn't send: the agent CLI failed (exit code 2): bad flag."
        );
        let bad = send_error(AgentError::SchemaMismatch {
            errors: vec!["x".to_owned()],
        });
        assert_eq!(bad.kind, "agent-bad-reply");
        let start = send_error(AgentError::CouldNotStart {
            reason: "no".to_owned(),
        });
        assert_eq!(start.kind, "agent-could-not-start");
        // The generic wording, for runs that are not a Sync, is unchanged.
        assert_eq!(
            agent_error(AgentError::Cancelled).message,
            "the run was cancelled"
        );
    }
}
