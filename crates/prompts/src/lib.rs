//! Prompt assembly for meet-ai.
//!
//! Phase 4 (SPEC §5): the wrap-up prompt, the notes schema and the Start Work prompt.
//! Also the Push Ticket prompt for the Sync run, and the check on its reply.
//!
//! L9 and L10 are the whole design: this app makes **no AI calls**. It renders a
//! self-contained prompt from a user-editable `minijinja` template in
//! `.app/prompts/*.md`. `crates/agent` passes it to the user's own agent CLI in
//! the background, or the UI puts it on the clipboard for Start Work and when
//! no agent is installed (SPEC A11). Nothing in this crate may grow a network
//! client.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod notes;
pub mod push_ticket;
pub mod start_work;
mod template;
pub mod wrap_up;

pub use notes::{NOTES_SCHEMA, Notes, Task, notes_schema};
pub use push_ticket::{PushTicketInput, SYNC_SCHEMA, Synced, parse_sync_reply, sync_schema};
pub use start_work::{StartWorkInput, transcript_excerpt};
pub use wrap_up::{Target, WrapUpInput};

/// The prompt buttons the UI offers (SPEC §5, Phase 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// Summarize the meeting into `meeting.md` and draft tickets.
    WrapUp,
    /// Start work on a ticket in the user's repo.
    StartWork,
    /// Push a drafted ticket to the user's tracker, using the agent's own
    /// connections (L11 — this app stores zero tokens). The Sync run.
    PushTicket,
}

impl PromptKind {
    /// The template filename under `.app/prompts/`.
    pub fn template_name(self) -> &'static str {
        match self {
            PromptKind::WrapUp => "wrap-up.md",
            PromptKind::StartWork => "start-work.md",
            PromptKind::PushTicket => "push-ticket.md",
        }
    }
}

/// Everything that can go wrong rendering a prompt.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The user edited a template into something minijinja cannot parse.
    ///
    /// Templates are deliberately user-editable, so this is an expected state,
    /// not a bug. The UI shows the template name and the parse message.
    #[error("the `{template}` prompt template has a syntax error: {detail}")]
    Template { template: String, detail: String },

    /// The template file could not be read.
    #[error("could not read the prompt template")]
    Io(#[from] std::io::Error),

    /// The agent sent back something that is not valid notes JSON: not JSON at
    /// all, or JSON that fails [`NOTES_SCHEMA`]. Nothing may be written from it
    /// (SPEC A11, "Guarding against the meeting itself").
    #[error("the agent's notes do not match the notes schema: {}", problems.join("; "))]
    InvalidNotes { problems: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prompt_kind_names_a_template() {
        for kind in [
            PromptKind::WrapUp,
            PromptKind::StartWork,
            PromptKind::PushTicket,
        ] {
            assert!(kind.template_name().ends_with(".md"));
        }
    }
}
