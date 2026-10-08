//! "Send a test ticket" in Settings, Tracker (TUR-113): a check that the
//! user's agent can reach their tracker through the MCP server they picked,
//! **without creating anything**.
//!
//! One background agent run, like a Sync run, with only the tools of that
//! server (`mcp__<server>__*`). The prompt asks for one read: the name of the
//! team, project or repository new issues would go to. It gets nothing from
//! any meeting or ticket. The agent CLI cannot be limited to the server's
//! read-only tools by name (each server names its tools its own way), so
//! "read only" rests on the prompt; the run cannot reach anything but that
//! server.
//!
//! The answer is worded for the window: the project's name, or the same
//! causes and kinds as a failed Sync ([`super::errors`]), each ending in what
//! to do next.

use std::sync::LazyLock;

use agent::{CancelHandle, Harness, Job};
use prompts::push_ticket::refused_reason;
use serde::Serialize;
use serde_json::Value;

use super::errors::{self, SYNC_REFUSED, SYNC_UNREACHABLE};
use super::tracker::checked;
use super::{RunSettings, harness_for, tracker_name};
use crate::config::{self, TicketsConfig};
use crate::error::{UiError, on_blocking_pool};

/// The check's reply schema, strict like the Sync one so Codex accepts it.
const CHECK_SCHEMA: &str = r#"{
  "type": "object",
  "description": "What the read-only check found in the tracker.",
  "properties": {
    "project": {
      "type": ["string", "null"],
      "description": "The name of the team, project or repository new issues would go to, as the tracker shows it. null if the tracker could not be read."
    },
    "refused_reason": {
      "type": ["string", "null"],
      "description": "When the tracker itself said no (no permission, nothing found), its reason in a few plain words. null otherwise, including when the tracker could not be reached."
    }
  },
  "required": ["project", "refused_reason"],
  "additionalProperties": false
}"#;

/// The longest project name passed on, in characters.
const MAX_PROJECT_CHARS: usize = 100;

/// What the check found, in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackerCheck {
    /// The team, project or repository new tickets would go to.
    pub project: String,
    /// The sentence to show, e.g. "Claude Code reached Linear. New tickets
    /// will go to Engineering."
    pub message: String,
}

/// Check that the agent reaches `tracker` through `tracker_mcp`, the values
/// on screen (saved or not). Creates nothing. Takes as long as the agent
/// does (up to `agent.timeout_sec`), so it runs on the blocking pool.
#[tauri::command]
#[specta::specta]
pub async fn send_test_ticket(
    tracker: String,
    tracker_mcp: String,
) -> Result<TrackerCheck, UiError> {
    on_blocking_pool(move || {
        let tickets = checked(&tracker, &tracker_mcp)?;
        let agent = config::agent()?;
        let harness = harness_for(&agent).map_err(again)?;
        check(
            harness.as_ref(),
            &RunSettings::new(&agent, tickets),
            &CancelHandle::new(),
        )
    })
    .await?
}

/// [`send_test_ticket`] with a given harness and settings.
pub(crate) fn check(
    harness: &dyn Harness,
    settings: &RunSettings,
    cancel: &CancelHandle,
) -> Result<TrackerCheck, UiError> {
    let tickets = &settings.tickets;
    let tools = agent::mcp::tracker_tools(harness.id(), &tickets.tracker_mcp);
    if tools.is_empty() {
        return Err(UiError::app(
            "sync-no-tracker",
            "Pick your tracker connection (also called an MCP server) before sending a test ticket.",
        ));
    }
    let mut job = Job::sync(prompt(tickets), schema(), tools);
    job.model = settings.model.clone();
    job.timeout = settings.timeout;
    job.cancel = cancel.clone();
    let reply = harness
        .run(&job)
        .map_err(|error| again(errors::send_error(error)))?;
    read_reply(tickets, harness.id(), &reply)
}

/// What the check's reply says, as [`TrackerCheck`] or an error by cause.
fn read_reply(
    tickets: &TicketsConfig,
    harness_id: &str,
    reply: &Value,
) -> Result<TrackerCheck, UiError> {
    let tracker = tracker_name(&tickets.tracker);
    let agent = errors::harness_name(harness_id);
    if let Some(project) = project_in(reply) {
        return Ok(TrackerCheck {
            message: format!(
                "{} reached {tracker}. New tickets will go to {project}.",
                capitalized(agent)
            ),
            project,
        });
    }
    if let Some(reason) = refused_reason(reply) {
        return Err(UiError::app(
            SYNC_REFUSED,
            format!(
                "{tracker} said no: {reason}. Check the project and your access in {tracker}, then send the test ticket again."
            ),
        ));
    }
    Err(UiError::app(
        SYNC_UNREACHABLE,
        format!(
            "Your agent couldn't reach {tracker}. Check that \"{}\" is connected and signed in in {agent}, then send the test ticket again.",
            tickets.tracker_mcp
        ),
    ))
}

/// The reply's project name: folded onto one line, cut to
/// [`MAX_PROJECT_CHARS`]. `None` when blank.
fn project_in(reply: &Value) -> Option<String> {
    let folded = reply
        .get("project")?
        .as_str()?
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if folded.is_empty() || folded.eq_ignore_ascii_case("null") {
        return None;
    }
    if folded.chars().count() <= MAX_PROJECT_CHARS {
        return Some(folded);
    }
    let cut: String = folded.chars().take(MAX_PROJECT_CHARS - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// A Sync error's "then press Retry." as the check's own next step.
fn again(error: UiError) -> UiError {
    let message = error
        .message
        .replace(
            "Sending stopped before it finished. Press Retry.",
            "The check stopped before it finished. Send the test ticket again.",
        )
        .replace("then press Retry.", "then send the test ticket again.")
        .replace("Couldn't send: ", "Couldn't check: ");
    UiError { message, ..error }
}

/// `text` with its first letter in upper case, for a name that starts a
/// sentence ("your agent").
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The check's prompt. Nothing from a meeting or a ticket is in it.
fn prompt(tickets: &TicketsConfig) -> String {
    let tracker = tracker_name(&tickets.tracker);
    let server = tickets.tracker_mcp.trim();
    format!(
        "You are running in the background for meet-ai. There is no window and nobody to answer questions. \
Your one job is a read-only check that you can reach the user's {tracker} tracker through the MCP server named \"{server}\". \
Use only that server's tools, and only to read. Do not create, change, comment on, move or delete anything.\n\n\
Make one call that reads the team, project or repository new issues would go to: the user's default, or the only one there is. \
If there are several and none is the default, take the first.\n\n\
Reply with only this JSON and nothing else:\n\n\
{{\"project\": \"<its name as {tracker} shows it>\", \"refused_reason\": null}}\n\n\
If you could not read it, set \"project\" to null, and set \"refused_reason\" like this:\n\n\
- {tracker} answered and said no (no permission, nothing found): its reason in a few plain words.\n\
- The tool is missing, the server needs sign-in, or it could not be reached: null.\n\n\
Never make up a name or a reason.\n"
    )
}

/// [`CHECK_SCHEMA`], parsed.
fn schema() -> Value {
    static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
        // quality: allow-unwrap CHECK_SCHEMA is a literal, parsed by a unit test
        serde_json::from_str(CHECK_SCHEMA).expect("CHECK_SCHEMA is valid JSON")
    });
    SCHEMA.clone()
}

#[cfg(test)]
mod tests;
