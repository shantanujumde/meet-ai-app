//! Looking for Claude Code and Codex, and turning what was found into the
//! rows the Setup screen lists.
//!
//! The looking itself is `agent::detect`. This part decides what the screen
//! says about it: ready, signed out or missing, the sign-in command to show,
//! and the models to offer (TUR-74: every model the CLI offers, and which
//! one it picks on its own when its settings say).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use agent::{ClaudeHarness, CodexHarness, Harness as _, Install, ListCache};

use super::{AgentChoice, AgentCli, AgentCliId, AgentCliState, AgentHarness, AgentModel};
use crate::error::UiError;
use crate::platform::{SIGN_IN_SHELL, SignInShell};

/// Codex's own model list, per binary, so opening Settings again does not
/// start Codex each time (up to 15 s).
static CODEX_MODELS: LazyLock<ListCache> =
    LazyLock::new(|| ListCache::new(agent::models::LIST_CACHE_TTL));

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

    /// The command typed in PowerShell / Terminal: `claude`, `codex`.
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

    /// The harness id `agent::models` keys its lists by.
    fn harness_id(self) -> &'static str {
        match self {
            Self::ClaudeCode => agent::claude::ID,
            Self::Codex => agent::codex::ID,
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
    // `models.json`; nothing is run for it, and the settings file is only read.
    let models = ClaudeHarness::new().models();
    let mut cli = cli_view(AgentCliId::ClaudeCode, install, models);
    cli.cli_default = agent::claude::settings_model();
    cli
}

/// Codex, plus its own model list when it can be asked for one: found and
/// signed in. Asking runs `codex debug models`, up to 15 s, kept for
/// [`agent::models::LIST_CACHE_TTL`]. When Codex cannot be asked, the list
/// in `models.json`.
fn detect_codex(binary_path: Option<&Path>) -> AgentCli {
    let install = agent::detect::codex(binary_path);
    let listed = match &install {
        Some(install) if install.signed_in => CODEX_MODELS.get_or_list(&install.path, || {
            CodexHarness::with_binary(&install.path).models()
        }),
        _ => Vec::new(),
    };
    cli_view(AgentCliId::Codex, install, codex_models(listed))
}

/// What Codex listed, or meet-ai's own list when it listed nothing.
pub(super) fn codex_models(listed: Vec<String>) -> Vec<String> {
    if !listed.is_empty() {
        return listed;
    }
    agent::models::listed(agent::codex::ID)
        .into_iter()
        .map(|model| model.name)
        .collect()
}

/// The Setup screen's row for `id`, from what detection found. `models` are
/// names in the CLI's order; labels and notes come from `models.json`.
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
        models: agent::models::described(id.harness_id(), models)
            .into_iter()
            .map(AgentModel::from)
            .collect(),
        cli_default: None,
        can_test: install.is_some(),
    }
}

/// What to type in PowerShell / Terminal to sign in: `claude auth login`,
/// `codex login`, written for this OS's terminal.
pub(super) fn sign_in_command(id: AgentCliId, path: Option<&Path>) -> String {
    let path_var = std::env::var_os("PATH");
    sign_in_command_for(SIGN_IN_SHELL, id, path, path_var.as_deref())
}

/// [`sign_in_command`] for `shell`, with `path_var` as the app's `PATH`.
///
/// The bare name is shown when the terminal will find it. A copy it would
/// not (macOS: inside an app bundle, such as Codex in ChatGPT.app; Windows:
/// in a folder not on `PATH`) gets its full path, quoted for that shell.
pub(super) fn sign_in_command_for(
    shell: SignInShell,
    id: AgentCliId,
    path: Option<&Path>,
    path_var: Option<&OsStr>,
) -> String {
    let program = match (shell, path) {
        (SignInShell::PosixAppBundles, Some(path)) if in_app_bundle(path) => {
            shell_quote(&path.display().to_string())
        }
        (SignInShell::PowerShell, Some(path))
            if path.is_absolute() && !folder_on_path(path, path_var) =>
        {
            powershell_program(&path.display().to_string())
        }
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

/// Whether the folder holding `path` is on `path_var`. Windows folder names
/// are case-insensitive and may end in a `\\`.
fn folder_on_path(path: &Path, path_var: Option<&OsStr>) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    let normal = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    let dir = normal(dir);
    path_var.is_some_and(|var| std::env::split_paths(var).any(|entry| normal(&entry) == dir))
}

/// `text` as a PowerShell command: as is when it is plain, otherwise in
/// single quotes (a `'` inside doubled) behind the call operator `&`, since a
/// quoted string alone is just a string to PowerShell.
fn powershell_program(text: &str) -> String {
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "\\/._-:".contains(c));
    if plain && !text.is_empty() {
        return text.to_owned();
    }
    format!("& '{}'", text.replace('\'', "''"))
}

/// `text` as one shell word: as is when it is plain, otherwise in single
/// quotes, with any single quote inside written as `'\\''`.
fn shell_quote(text: &str) -> String {
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%".contains(c));
    if plain && !text.is_empty() {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}
