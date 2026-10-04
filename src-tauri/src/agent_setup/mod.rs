//! The Setup screens for the agent (SPEC A11, "The flow", Setup row): which
//! agent CLI writes the notes after a call, with which model, and a Test
//! button that runs a 3-line sample meeting through it end to end.
//!
//! The same four commands serve the onboarding step and Settings:
//!
//! - [`agent_choice`]: what `config.jsonc` says now.
//! - [`detect_agents`]: whether Claude Code and Codex are on this Mac and
//!   signed in ([`view`]).
//! - [`save_agent_choice`]: write the pick back, keeping the rest of the file.
//! - [`test_agent`]: the real notes run on a sample transcript ([`test_run`]).
//!
//! Settings alone has two more (TUR-101): [`notes_auto_run`] and
//! [`save_notes_auto_run`], whether notes start on their own after a call
//! (`agent.auto_run`) or only from the meeting's "Make notes now".
//!
//! The types below are what the window sends and gets back. They are kept
//! apart from [`AgentConfig`] on purpose: the screen sends a path as text and
//! never sees `auto_run` or `timeout_sec`, which saving the pick keeps as
//! they were. `auto_run` is saved on its own by [`save_notes_auto_run`], so a
//! pick and a flip of the Auto / Manual choice landing close together never
//! put each other's old value back.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager as _};

use crate::config::{self, AgentConfig, ConfigError, Harness};
use crate::error::{UiError, on_blocking_pool};
use crate::folder_move::FolderGate;

mod test_run;
mod view;

// --- what crosses the IPC boundary -----------------------------------------

/// `agent.harness`: which CLI runs the notes, or none (copy prompt instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AgentHarness {
    ClaudeCode,
    Codex,
    None,
}

/// The pick the Setup screen shows and saves: `agent.harness`, `agent.model`
/// and `agent.binary_path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentChoice {
    pub harness: AgentHarness,
    /// Any model name the CLI accepts. Blank means "Default": no `--model`,
    /// the CLI picks its own (SPEC A14), and `null` in `config.jsonc`.
    pub model: String,
    /// Where the CLI is, when the app cannot find it by itself. `null` (or
    /// blank) means "look for it".
    pub binary_path: Option<String>,
}

/// One of the agent CLIs the app knows how to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AgentCliId {
    ClaudeCode,
    Codex,
}

/// Whether a CLI can run right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum AgentCliState {
    /// Found, and signed in.
    Ready,
    /// Found, but nobody is signed in.
    SignedOut,
    /// Not found on this Mac.
    Missing,
}

/// One agent CLI as the Setup screen lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentCli {
    pub id: AgentCliId,
    /// "Claude Code" or "Codex".
    pub name: String,
    /// The company the transcript goes to, for the privacy sentence:
    /// "Anthropic" or "OpenAI".
    pub provider: String,
    pub state: AgentCliState,
    /// Where it was found. `null` when it was not.
    pub path: Option<String>,
    /// What `--version` printed, if anything usable.
    pub version: Option<String>,
    /// What to type in Terminal to sign in.
    pub sign_in_command: String,
    /// Models for the picker, in order: the first two are the suggestion
    /// buttons next to "Default", the rest go in the dropdown. The user can
    /// still type any other name.
    pub models: Vec<AgentModel>,
    /// The model the CLI runs when meet-ai passes none, when its own settings
    /// say which (Claude Code's `~/.claude/settings.json`). `null` when they
    /// do not; the CLI then uses its built-in default.
    pub cli_default: Option<String>,
    /// Whether the Test button can run. False only when the CLI is missing.
    pub can_test: bool,
}

/// One model the picker offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentModel {
    /// What is saved and passed as `--model`: `sonnet`, `gpt-5.6-terra`.
    pub name: String,
    /// What the screen shows: `Sonnet`. The name when there is no label.
    pub label: String,
    /// One line on when to pick it, if meet-ai's list has one.
    pub note: Option<String>,
}

impl From<agent::Model> for AgentModel {
    fn from(model: agent::Model) -> Self {
        Self {
            name: model.name,
            label: model.label,
            note: model.note,
        }
    }
}

/// One task from the test run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTestTask {
    pub title: String,
    pub owner: Option<String>,
    pub due: Option<String>,
}

/// What the test run wrote from the sample meeting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTestResult {
    pub summary: String,
    pub decisions: Vec<String>,
    pub open_questions: Vec<String>,
    pub tasks: Vec<AgentTestTask>,
    /// How long the run took, in whole seconds (rounded). A whole number so
    /// the wire type is a plain `number`; an `f64` would be `number | null`.
    pub seconds: u32,
}

// --- commands --------------------------------------------------------------

/// The agent pick in `config.jsonc`. An `agent.harness` the app does not know
/// is an `unknown-harness` error, so the screen can point at the picker.
#[tauri::command]
#[specta::specta]
pub async fn agent_choice() -> Result<AgentChoice, UiError> {
    on_blocking_pool(|| Ok(AgentChoice::from_config(&config::agent()?))).await?
}

/// Claude Code, then Codex: found or not, signed in or not. Each check can
/// take seconds (a login shell, `--version`, the sign-in check), so the two
/// run side by side.
#[tauri::command]
#[specta::specta]
pub async fn detect_agents(choice: AgentChoice) -> Result<Vec<AgentCli>, UiError> {
    on_blocking_pool(move || view::detect_all(&choice)).await?
}

/// Save the pick into `config.jsonc` and return it as saved. Writes under the
/// meetings root, so through the [`FolderGate`] like every other writer.
#[tauri::command]
#[specta::specta]
pub async fn save_agent_choice(
    app: AppHandle,
    choice: AgentChoice,
) -> Result<AgentChoice, UiError> {
    on_blocking_pool(move || app.state::<FolderGate>().writing(|| save(choice))).await?
}

/// `agent.auto_run`: true when notes start on their own after a call, false
/// when they start only from the meeting's "Make notes now" (TUR-101).
#[tauri::command]
#[specta::specta]
pub async fn notes_auto_run() -> Result<bool, UiError> {
    on_blocking_pool(|| Ok(config::agent()?.auto_run)).await?
}

/// Save `agent.auto_run`, keeping the rest of `agent`, and return it as read
/// back from disk. Through the [`FolderGate`] like every other writer.
#[tauri::command]
#[specta::specta]
pub async fn save_notes_auto_run(app: AppHandle, on: bool) -> Result<bool, UiError> {
    on_blocking_pool(move || {
        app.state::<FolderGate>().writing(|| {
            config::update_agent(|current| with_auto_run(on, current))?;
            Ok(config::agent()?.auto_run)
        })
    })
    .await?
}

/// Run the sample meeting through the picked CLI, the same way the notes run
/// after a call does, and return what came back.
#[tauri::command]
#[specta::specta]
pub async fn test_agent(choice: AgentChoice) -> Result<AgentTestResult, UiError> {
    on_blocking_pool(move || test_run::run(&choice)).await?
}

// --- between the screen and the config -------------------------------------

impl From<Harness> for AgentHarness {
    fn from(harness: Harness) -> Self {
        match harness {
            Harness::ClaudeCode => Self::ClaudeCode,
            Harness::Codex => Self::Codex,
            Harness::None => Self::None,
        }
    }
}

impl From<AgentHarness> for Harness {
    fn from(harness: AgentHarness) -> Self {
        match harness {
            AgentHarness::ClaudeCode => Self::ClaudeCode,
            AgentHarness::Codex => Self::Codex,
            AgentHarness::None => Self::None,
        }
    }
}

impl AgentChoice {
    /// The part of `agent` the screen shows.
    fn from_config(agent: &AgentConfig) -> Self {
        Self {
            harness: agent.harness.into(),
            model: agent.model.clone().unwrap_or_default(),
            binary_path: agent
                .binary_path
                .as_ref()
                .map(|path| path.display().to_string()),
        }
    }

    /// `binary_path`, with blank read as "not set".
    fn binary_path(&self) -> Option<PathBuf> {
        self.binary_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    }

    /// `model` as it is saved and run: trimmed, and `None` when blank, which
    /// passes no `--model` so the CLI picks its own (SPEC A14). The same for
    /// every harness; for `none` it is kept, so switching back finds it.
    fn model(&self) -> Option<String> {
        let model = self.model.trim();
        (!model.is_empty()).then(|| model.to_owned())
    }
}

/// The `agent` section to save: the screen's pick, plus `auto_run` and
/// `timeout_sec` from `current` (the section as it is now).
///
/// If `current` failed only because `agent.harness` names a CLI the app does
/// not know, those two are the defaults: saving a new pick is how the user
/// fixes that typo. Any other problem with the file is returned, and nothing
/// is written.
fn merged(
    choice: &AgentChoice,
    current: Result<AgentConfig, ConfigError>,
) -> Result<AgentConfig, ConfigError> {
    let current = match current {
        Ok(current) => current,
        Err(ConfigError::UnknownHarness(_)) => AgentConfig::default(),
        Err(error) => return Err(error),
    };
    Ok(AgentConfig {
        harness: choice.harness.into(),
        model: choice.model(),
        binary_path: choice.binary_path(),
        auto_run: current.auto_run,
        timeout_sec: current.timeout_sec,
    })
}

/// The `agent` section to save with `auto_run` set to `on` and everything
/// else as it is now. A file that cannot be read (an unknown harness too) is
/// refused rather than rewritten with defaults: only the agent picker may
/// replace the harness.
fn with_auto_run(
    on: bool,
    current: Result<AgentConfig, ConfigError>,
) -> Result<AgentConfig, ConfigError> {
    Ok(AgentConfig {
        auto_run: on,
        ..current?
    })
}

/// Write the pick and read it back, so the screen shows what is on disk.
/// The merge runs under the config write lock, from the file as it is then.
fn save(choice: AgentChoice) -> Result<AgentChoice, UiError> {
    config::update_agent(|current| merged(&choice, current))?;
    Ok(AgentChoice::from_config(&config::agent()?))
}

#[cfg(test)]
mod tests;
