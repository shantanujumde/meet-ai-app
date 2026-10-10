//! The user's agent CLI, found and ready to run, and what each way a run
//! fails is called. One copy for every runner (TUR-168): the notes run after
//! a call (`agent_run::notes`), Sync and its tracker check (`sync`), and
//! Setup's Test ([`super::test_run`]).
//!
//! Before this, each runner found the CLI and put its folder on `PATH` its
//! own way, and their error kinds and sign-in commands had drifted apart.
//! Each runner still words its own sentences ("Couldn't send: ...", "...
//! then press Retry."); the kind and the sign-in command come from here.

use std::path::{Path, PathBuf};

use agent::{AgentError, ClaudeHarness, CodexHarness, Harness, Install};

use super::{AgentCliId, view};
use crate::config::{AgentConfig, Harness as Pick};
use crate::error::UiError;

/// The picked CLI, found, as a harness to run.
pub(crate) struct Found {
    pub harness: Box<dyn Harness>,
    /// What to type in a terminal to sign it in, as Setup shows it
    /// ([`view::sign_in_command`]): the quoted full path for a copy the
    /// terminal would not find, such as Codex inside ChatGPT.app.
    pub sign_in: String,
}

/// Why there is no CLI to run.
#[derive(Debug)]
pub(crate) enum NotFound {
    /// `agent.harness` is `none`.
    NoAgent,
    /// Not installed, or (when asked to check) nobody is signed in to it.
    Agent {
        error: AgentError,
        /// What to type to sign it in.
        sign_in: String,
    },
}

/// The CLI `settings` picks (`agent.harness`, `agent.binary_path`), found and
/// ready to run.
///
/// With `check_sign_in`, the CLI is also asked for its version and sign-in
/// state (a few seconds) and a signed-out one is refused. Without it, only
/// the binary is looked for, and a signed-out CLI says so when it runs.
pub(crate) fn harness_for(settings: &AgentConfig, check_sign_in: bool) -> Result<Found, NotFound> {
    let id = cli_of(settings.harness).ok_or(NotFound::NoAgent)?;
    let binary_path = settings.binary_path.as_deref();
    let found = if check_sign_in {
        detect(id, binary_path).map(|install| (install.path, install.signed_in))
    } else {
        locate(id, binary_path).map(|path| (path, true))
    };
    let Some((path, signed_in)) = found else {
        return Err(NotFound::Agent {
            error: AgentError::NotInstalled {
                harness: id.name().to_owned(),
            },
            sign_in: view::sign_in_command(id, None),
        });
    };
    let sign_in = view::sign_in_command(id, Some(&path));
    if !signed_in {
        return Err(NotFound::Agent {
            error: AgentError::NotSignedIn {
                harness: id.name().to_owned(),
            },
            sign_in,
        });
    }
    Ok(Found {
        harness: harness_at(id, &path),
        sign_in,
    })
}

/// Where the CLI is, without running it: `binary_path` (`agent.binary_path`),
/// then the login shell, then the usual install folders
/// (`agent::detect::find`).
pub(crate) fn locate(id: AgentCliId, binary_path: Option<&Path>) -> Option<PathBuf> {
    let cli = match id {
        AgentCliId::ClaudeCode => &agent::detect::CLAUDE,
        AgentCliId::Codex => &agent::detect::CODEX,
    };
    agent::detect::find(cli, &agent::detect::Lookup::system(binary_path))
}

/// [`locate`], plus the CLI's version and sign-in state.
fn detect(id: AgentCliId, binary_path: Option<&Path>) -> Option<Install> {
    match id {
        AgentCliId::ClaudeCode => agent::detect::claude(binary_path),
        AgentCliId::Codex => agent::detect::codex(binary_path),
    }
}

/// The harness that runs the CLI at `path`, with that CLI's own folder first
/// on the child's `PATH`: an npm install is a `#!/usr/bin/env node` script,
/// and an app not opened from a terminal does not have that folder on its
/// `PATH`.
fn harness_at(id: AgentCliId, path: &Path) -> Box<dyn Harness> {
    match id {
        // `CodexHarness` puts the CLI's own folder first on `PATH` itself.
        AgentCliId::Codex => Box::new(CodexHarness::with_binary(path)),
        AgentCliId::ClaudeCode => {
            let harness = ClaudeHarness::new().with_binary(path);
            Box::new(
                match path.parent().and_then(agent::process::search_path_with) {
                    Some(search_path) => harness.with_search_path(search_path),
                    None => harness,
                },
            )
        }
    }
}

/// The CLI `agent.harness` names; `None` for `none`.
pub(crate) fn cli_of(pick: Pick) -> Option<AgentCliId> {
    match pick {
        Pick::ClaudeCode => Some(AgentCliId::ClaudeCode),
        Pick::Codex => Some(AgentCliId::Codex),
        Pick::None => None,
    }
}

/// The CLI a harness id (`agent::Harness::id`) is; `None` for the fake one
/// tests use.
pub(crate) fn cli_with_id(harness_id: &str) -> Option<AgentCliId> {
    match harness_id {
        agent::claude::ID => Some(AgentCliId::ClaudeCode),
        agent::codex::ID => Some(AgentCliId::Codex),
        _ => None,
    }
}

/// The name the user knows the harness with id `harness_id` by: "Claude
/// Code", "Codex". `None` for one the app does not know.
pub(crate) fn display_name(harness_id: &str) -> Option<&'static str> {
    cli_with_id(harness_id).map(AgentCliId::name)
}

/// The sign-in command for a harness when where it is is not known: its bare
/// name, as Setup shows it for a CLI on `PATH`. `harness` is its id
/// (`agent::Harness::id`) or its name as an [`AgentError`] gives it.
pub(crate) fn bare_sign_in(harness: &str) -> Option<String> {
    let named = [AgentCliId::ClaudeCode, AgentCliId::Codex]
        .into_iter()
        .find(|id| id.name() == harness);
    cli_with_id(harness)
        .or(named)
        .map(|id| view::sign_in_command(id, None))
}

/// Every way an agent run can fail, one per thing the user can do about it.
/// The one mapping from [`AgentError`]; every runner's kind comes from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    NotInstalled,
    NotSignedIn,
    TimedOut,
    Cancelled,
    Failed,
    InvalidJson,
    SchemaMismatch,
    CouldNotStart,
}

impl ErrorKind {
    /// Every kind, for tests that check each runner maps them the same.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 8] = [
        Self::NotInstalled,
        Self::NotSignedIn,
        Self::TimedOut,
        Self::Cancelled,
        Self::Failed,
        Self::InvalidJson,
        Self::SchemaMismatch,
        Self::CouldNotStart,
    ];

    pub(crate) fn of(error: &AgentError) -> Self {
        match error {
            AgentError::NotInstalled { .. } => Self::NotInstalled,
            AgentError::NotSignedIn { .. } => Self::NotSignedIn,
            AgentError::TimedOut { .. } => Self::TimedOut,
            AgentError::Cancelled => Self::Cancelled,
            AgentError::CliFailed { .. } => Self::Failed,
            AgentError::InvalidJson { .. } => Self::InvalidJson,
            AgentError::SchemaMismatch { .. } => Self::SchemaMismatch,
            AgentError::CouldNotStart { .. } => Self::CouldNotStart,
        }
    }

    /// The `app` error kind the window switches on (`src/ipc/errors.ts`).
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::NotInstalled => "agent-not-installed",
            Self::NotSignedIn => "agent-not-signed-in",
            Self::TimedOut => "agent-timed-out",
            Self::Cancelled => "agent-cancelled",
            Self::Failed => "agent-failed",
            Self::InvalidJson => "agent-invalid-json",
            Self::SchemaMismatch => "agent-schema-mismatch",
            Self::CouldNotStart => "agent-could-not-start",
        }
    }
}

/// An agent run's error as the window gets it: its [`ErrorKind::key`] and
/// the error's own sentence. A signed-out CLI's sentence gives `sign_in`, the
/// command that signs it in, when there is one.
pub(crate) fn agent_error(error: AgentError, sign_in: Option<&str>) -> UiError {
    let kind = ErrorKind::of(&error).key();
    let message = match (&error, sign_in) {
        (AgentError::NotSignedIn { harness }, Some(command)) => format!(
            "{harness} is not signed in. Open a terminal and run {command}, then try again."
        ),
        _ => error.to_string(),
    };
    UiError::app(kind, message)
}

#[cfg(test)]
pub(crate) mod tests;
