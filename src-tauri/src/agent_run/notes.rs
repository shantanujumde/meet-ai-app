//! One notes run, start to finish, with no Tauri in it: read the meeting,
//! build the prompt, run the agent, check its answer, write the files.
//!
//! The meeting is read first, so a meeting marked `agent_notes: off` or one
//! with no transcript fails before any agent is looked for, let alone sent
//! anything (SPEC A11, "Skip one meeting").

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent::{AgentError, CancelHandle, ClaudeHarness, CodexHarness, Harness, Install, Job};
use prompts::wrap_up::{Target, WrapUpInput, render_wrap_up_from};
use store::agent_notes::{self, Analysis, AnalyzedBy};
use store::folder_name::{prettify_slug, split_folder_name};
use store::meeting::Meeting;
use store::watcher::SelfWrites;

use super::Failure;
use super::failure;
use crate::config;

/// The agent a run uses, found and ready to start.
pub struct Agent {
    /// A harness whose CLI was found and is signed in.
    pub harness: Box<dyn Harness>,
    /// `agent.model`; `None` lets the CLI pick.
    pub model: Option<String>,
    /// `agent.timeout_sec`.
    pub timeout: Duration,
    /// Where the run's empty working folder is made.
    pub work_root: PathBuf,
}

/// A finished run's write: given the meetings root, writes the notes there
/// and returns how many tasks it wrote.
pub type SaveWrite<'a> = &'a dyn Fn(&Path) -> Result<u32, Failure>;

/// Writes the notes for `meeting_id` under `root` and returns how many tasks
/// were written. `agent` is asked for the CLI only once the meeting is known
/// to have a transcript and notes switched on.
///
/// `root` is where the meeting is read from at the start. The answer is
/// written through `save`, which runs the write against the meetings root as
/// it is once the answer is in: the user may have moved the meetings folder
/// while the agent worked (TUR-17).
pub fn run_notes(
    root: &Path,
    meeting_id: &str,
    agent: impl FnOnce() -> Result<Agent, Failure>,
    cancel: &CancelHandle,
    self_writes: &SelfWrites,
    save: impl FnOnce(SaveWrite<'_>) -> Result<u32, Failure>,
) -> Result<u32, Failure> {
    let prompt = prepare(root, meeting_id)?;
    let agent = agent()?;
    let harness_id = agent.harness.id();

    let mut job = Job::notes(prompt, prompts::notes_schema().clone());
    job.model.clone_from(&agent.model);
    job.timeout = agent.timeout;
    job.work_root = agent.work_root;
    job.cancel = cancel.clone();
    let reply = agent.harness.run(&job).map_err(|error| match error {
        AgentError::Cancelled => stopped(root, meeting_id, harness_id),
        other => failure::from_agent(&other, harness_id),
    })?;
    let notes = prompts::Notes::from_value(reply).map_err(|_| failure::bad_reply())?;
    // Cancel pressed while the answer was on its way: write nothing.
    if cancel.is_cancelled() {
        return Err(stopped(root, meeting_id, harness_id));
    }

    let analysis = Analysis {
        by: analyzed_by(harness_id),
        model: agent.model.unwrap_or_default(),
        at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
    };
    save(&|root| {
        let outcome = agent_notes::write(root, meeting_id, &notes, &analysis, self_writes)
            .map_err(failure::write_failed)?;
        if outcome.notes_off {
            // Switched off while the agent was working.
            return Err(failure::notes_off());
        }
        Ok(u32::try_from(outcome.written.len()).unwrap_or(u32::MAX))
    })
}

/// The agent the config names, found through its harness.
pub fn configured() -> Result<Agent, Failure> {
    let settings = config::agent()
        .map_err(|error| failure::could_not_start(crate::error::UiError::from(error).message))?;
    from_settings(&settings)
}

/// The agent `settings` names, found through its harness.
pub fn from_settings(settings: &config::AgentConfig) -> Result<Agent, Failure> {
    let harness: Box<dyn Harness> = match settings.harness {
        config::Harness::None => return Err(failure::no_agent()),
        config::Harness::Codex => {
            let finder = match &settings.binary_path {
                Some(path) => CodexHarness::with_binary(path),
                None => CodexHarness::new(),
            };
            // `CodexHarness` puts the CLI's own folder first on `PATH` itself.
            Box::new(detected(finder, |install| {
                CodexHarness::with_binary(install.path)
            })?)
        }
        config::Harness::ClaudeCode => {
            let mut finder = ClaudeHarness::new();
            if let Some(path) = &settings.binary_path {
                finder = finder.with_binary(path);
            }
            Box::new(detected(finder, |install| {
                // The CLI's own folder first on `PATH`: an npm install is a
                // `#!/usr/bin/env node` script, and an app opened from
                // Finder does not have that folder on its `PATH`.
                let search_path = install
                    .path
                    .parent()
                    .and_then(agent::process::search_path_with);
                let harness = ClaudeHarness::new().with_binary(install.path);
                match search_path {
                    Some(path) => harness.with_search_path(path),
                    None => harness,
                }
            })?)
        }
    };
    Ok(Agent {
        harness,
        // Already trimmed, and `None` for blank, by the config reader.
        model: settings.model.clone(),
        timeout: Duration::from_secs(settings.timeout_sec),
        work_root: std::env::temp_dir(),
    })
}

/// Finds `harness`'s CLI with its own `detect` and builds the harness to run
/// from what it found. Not there, or nobody signed in, is a failure.
pub fn detected<H: Harness>(harness: H, run_with: impl FnOnce(Install) -> H) -> Result<H, Failure> {
    let id = harness.id();
    let name = failure::display_name(id).to_owned();
    match harness.detect() {
        None => Err(failure::from_agent(
            &AgentError::NotInstalled { harness: name },
            id,
        )),
        Some(install) if !install.signed_in => Err(failure::from_agent(
            &AgentError::NotSignedIn { harness: name },
            id,
        )),
        Some(install) => Ok(run_with(install)),
    }
}

/// `analyzed_by` for a harness id. Anything but Codex is Claude Code, which
/// is what the fake harness in tests stands in for.
fn analyzed_by(harness_id: &str) -> AnalyzedBy {
    if harness_id == agent::codex::ID {
        AnalyzedBy::Codex
    } else {
        AnalyzedBy::ClaudeCode
    }
}

/// Everything read from the meeting folder, rendered into the agent's prompt.
fn prepare(root: &Path, meeting_id: &str) -> Result<String, Failure> {
    let dir = store::folder::meeting_dir(root, meeting_id).map_err(failure::could_not_start)?;
    if !dir.is_dir() {
        return Err(failure::no_transcript());
    }
    let meeting = Meeting::read(&dir.join(store::MEETING_FILE)).map_err(|error| {
        failure::could_not_start(format!(
            "{} could not be read: {error}",
            store::MEETING_FILE
        ))
    })?;
    if meeting.as_ref().is_some_and(store::notes_switch::is_off) {
        return Err(failure::notes_off());
    }
    let transcript = match std::fs::read_to_string(dir.join(store::TRANSCRIPT_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(failure::could_not_start(format!(
                "{} could not be read: {error}",
                store::TRANSCRIPT_FILE
            )));
        }
    };
    if transcript.trim().is_empty() {
        return Err(failure::no_transcript());
    }
    let notes = store::notes::read(&dir).map_err(|error| {
        failure::could_not_start(format!("{} could not be read: {error}", store::NOTES_FILE))
    })?;
    let input = WrapUpInput {
        title: title(meeting.as_ref(), meeting_id),
        date: date(meeting.as_ref(), meeting_id),
        transcript,
        notes,
    };
    render_wrap_up_from(root, &input, &Target::Agent)
        .map_err(|error| failure::could_not_start(format!("the prompt template: {error}")))
}

/// Why a cancelled run stopped: Cancel, or the user switching notes off for
/// this meeting, which cancels its run (TUR-12).
fn stopped(root: &Path, meeting_id: &str, harness_id: &str) -> Failure {
    if switched_off(root, meeting_id) {
        failure::notes_off()
    } else {
        failure::from_agent(&AgentError::Cancelled, harness_id)
    }
}

/// The meeting's `meeting.md` now says `agent_notes: off`.
pub fn switched_off(root: &Path, meeting_id: &str) -> bool {
    store::folder::meeting_dir(root, meeting_id)
        .ok()
        .and_then(|dir| Meeting::read(&dir.join(store::MEETING_FILE)).ok().flatten())
        .is_some_and(|meeting| store::notes_switch::is_off(&meeting))
}

/// The meeting's title, or its folder name made readable, as Copy prompt
/// does it.
fn title(meeting: Option<&Meeting>, meeting_id: &str) -> String {
    meeting.and_then(Meeting::title).unwrap_or_else(|| {
        let (_, _, slug) = split_folder_name(meeting_id);
        slug.map(prettify_slug)
            .unwrap_or_else(|| meeting_id.to_owned())
    })
}

/// The meeting's `date`, or the day and time from its folder name.
fn date(meeting: Option<&Meeting>, meeting_id: &str) -> String {
    let (day, time, _) = split_folder_name(meeting_id);
    meeting
        .and_then(Meeting::date)
        .or_else(|| day.map(|day| format!("{day} {}", time.unwrap_or_default())))
        .unwrap_or_default()
        .trim()
        .to_owned()
}
