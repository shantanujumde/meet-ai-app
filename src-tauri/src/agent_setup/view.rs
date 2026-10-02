//! Looking for Claude Code and Codex, and turning what was found into the
//! rows the Setup screen lists.
//!
//! The looking itself is `agent::detect`. This part decides what the screen
//! says about it: ready, signed out or missing, the sign-in command to show,
//! and the models to offer.

use std::path::{Path, PathBuf};

use agent::{ClaudeHarness, CodexHarness, Harness as _, Install};

use super::{AgentChoice, AgentCli, AgentCliId, AgentCliState, AgentHarness};
use crate::error::UiError;

impl AgentCliId {
    /// The name the user knows it by.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => agent::claude::DISPLAY_NAME,
            Self::Codex => "Codex",
        }
    }

    /// The company whose servers the transcript goes to.
    fn provider(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Anthropic",
            Self::Codex => "OpenAI",
        }
    }

    /// The command typed in Terminal: `claude`, `codex`.
    fn command(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    /// What follows the command to sign in.
    fn sign_in_args(self) -> &'static str {
        match self {
            Self::ClaudeCode => "auth login",
            Self::Codex => "login",
        }
    }

    /// The model used when the user picks none. Codex has none of ours: it
    /// picks its own.
    fn default_model(self) -> Option<String> {
        match self {
            Self::ClaudeCode => Some(agent::claude::DEFAULT_MODEL.to_owned()),
            Self::Codex => None,
        }
    }
}

/// Both CLIs, Claude Code first. Codex is looked for on a second thread
/// while this one looks for Claude Code, since each can take seconds.
pub(super) fn detect_all(choice: &AgentChoice) -> Result<Vec<AgentCli>, UiError> {
    let (claude_path, codex_path) = binary_paths(choice);
    std::thread::scope(|scope| {
        let codex = scope.spawn(|| detect_codex(codex_path.as_deref()));
        let claude = detect_claude(claude_path.as_deref());
        let codex = codex.join().map_err(|_| {
            UiError::app(
                "task-failed",
                "Looking for Codex stopped with an internal error.",
            )
        })?;
        Ok(vec![claude, codex])
    })
}

/// `choice.binary_path`, handed only to the CLI the user picked: it is that
/// CLI's path, and given to both they would report the same binary. For
/// `none`, neither gets it.
pub(super) fn binary_paths(choice: &AgentChoice) -> (Option<PathBuf>, Option<PathBuf>) {
    let path = choice.binary_path();
    match choice.harness {
        AgentHarness::ClaudeCode => (path, None),
        AgentHarness::Codex => (None, path),
        AgentHarness::None => (None, None),
    }
}

fn detect_claude(binary_path: Option<&Path>) -> AgentCli {
    let install = agent::detect::claude(binary_path);
    // A fixed list; nothing is run for it.
    let models = ClaudeHarness::new().models();
    cli_view(AgentCliId::ClaudeCode, install, models)
}

/// Codex, plus its model list when it can be asked for one: found and
/// signed in. Asking runs `codex debug models`, up to 15 s.
fn detect_codex(binary_path: Option<&Path>) -> AgentCli {
    let install = agent::detect::codex(binary_path);
    let models = match &install {
        Some(install) if install.signed_in => CodexHarness::with_binary(&install.path).models(),
        _ => Vec::new(),
    };
    cli_view(AgentCliId::Codex, install, models)
}

/// The Setup screen's row for `id`, from what detection found.
pub(super) fn cli_view(id: AgentCliId, install: Option<Install>, models: Vec<String>) -> AgentCli {
    let state = match &install {
        None => AgentCliState::Missing,
        Some(install) if install.signed_in => AgentCliState::Ready,
        Some(_) => AgentCliState::SignedOut,
    };
    let path = install.as_ref().map(|install| install.path.as_path());
    AgentCli {
        id,
        name: id.name().to_owned(),
        provider: id.provider().to_owned(),
        state,
        path: path.map(|path| path.display().to_string()),
        version: install.as_ref().and_then(|install| install.version.clone()),
        sign_in_command: sign_in_command(id, path),
        models,
        default_model: id.default_model(),
        can_test: install.is_some(),
    }
}

/// What to type in Terminal to sign in: `claude auth login`, `codex login`.
///
/// A copy inside an app bundle (Codex in ChatGPT.app, Claude Code in the
/// Claude app) is not on the shell's `PATH`, so the bare name would not be
/// found. That one gets its full path, quoted when it needs it.
pub(super) fn sign_in_command(id: AgentCliId, path: Option<&Path>) -> String {
    let program = match path {
        Some(path) if in_app_bundle(path) => shell_quote(&path.display().to_string()),
        _ => id.command().to_owned(),
    };
    format!("{program} {}", id.sign_in_args())
}

/// Whether some folder on `path` is a macOS app bundle (`Something.app`).
fn in_app_bundle(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .any(|dir| dir.extension().is_some_and(|ext| ext == "app"))
}

/// `text` as one shell word: as is when it is plain, otherwise in single
/// quotes, with any single quote inside written as `'\''`.
fn shell_quote(text: &str) -> String {
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c));
    if plain && !text.is_empty() {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}
