//! Reading and writing the `agent` and `tickets` sections of `config.jsonc`.
//!
//! Writes go through jsonc-parser's CST (concrete syntax tree: the parsed file
//! with every comment and blank line kept), and change only the keys of the
//! one section being saved. The user's comments, key order and any keys the
//! app does not know survive a save; the file is never re-serialized from a
//! struct.
//!
//! Each write also puts `config.schema.json` next to the file, so the
//! `"$schema": "./config.schema.json"` line gives editors autocomplete.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstRootNode};
use stt::registry::Preference;

use super::FILE;
use super::agent_section::parse_default_repo;
use super::agent_section::{AgentConfig, ConfigError, TicketsConfig, parse_agent, parse_tickets};

/// The JSON Schema for `config.jsonc`, kept in the repo and shipped inside the
/// binary.
pub const SCHEMA: &str = include_str!("../../config.schema.json");
pub(super) const SCHEMA_FILE: &str = "config.schema.json";

/// `agent` from `~/Meetings/.app/config.jsonc`. A missing file or section is
/// the SPEC §3.5 defaults; an unknown `harness` is an error.
#[allow(dead_code)] // TUR-9 (Setup screens) adds the IPC command that calls this.
pub fn agent() -> Result<AgentConfig, ConfigError> {
    parse_agent(&read_in(&app_dir()?)?)
}

/// `tickets` from `~/Meetings/.app/config.jsonc`, defaults when missing.
#[allow(dead_code)] // TUR-9 (Setup screens) adds the IPC command that calls this.
pub fn tickets() -> Result<TicketsConfig, ConfigError> {
    parse_tickets(&read_in(&app_dir()?)?)
}

/// `repos.default` from `~/Meetings/.app/config.jsonc`, `None` when unset.
pub fn default_repo() -> Result<Option<String>, ConfigError> {
    parse_default_repo(&read_in(&app_dir()?)?)
}

/// Save `agent` into `~/Meetings/.app/config.jsonc`, keeping everything else.
#[allow(dead_code)] // TUR-9 (Setup screens) adds the IPC command that calls this.
pub fn set_agent(agent: &AgentConfig) -> Result<(), ConfigError> {
    write_in(&app_dir()?, |raw| with_agent(raw, agent))
}

/// Save `tickets` into `~/Meetings/.app/config.jsonc`, keeping everything else.
#[allow(dead_code)] // TUR-9 (Setup screens) adds the IPC command that calls this.
pub fn set_tickets(tickets: &TicketsConfig) -> Result<(), ConfigError> {
    write_in(&app_dir()?, |raw| with_tickets(raw, tickets))
}

/// Save `transcription.engine` and `transcription.model` into
/// `~/Meetings/.app/config.jsonc` (TUR-75), keeping everything else —
/// `language`, `live` and the user's comments included. Read again by the next
/// recording; one already running keeps the engine it opened with.
pub fn set_transcription(engine: Preference, model: &str) -> Result<(), ConfigError> {
    write_in(&app_dir()?, |raw| with_transcription(raw, engine, model))
}

/// `raw` with `transcription.engine` and `transcription.model` set.
pub fn with_transcription(
    raw: &str,
    engine: Preference,
    model: &str,
) -> Result<String, ConfigError> {
    let engine = match engine {
        Preference::Auto => "auto",
        Preference::AppleSpeech => "apple-speech",
        Preference::Whisper => "whisper",
    };
    with_section(
        raw,
        "transcription",
        vec![("engine", engine.into()), ("model", model.into())],
    )
}

fn app_dir() -> Result<PathBuf, ConfigError> {
    super::app_dir().map_err(ConfigError::Root)
}

/// The text of `config.jsonc` in `dir`, or `""` if there is none yet.
pub(super) fn read_in(dir: &Path) -> Result<String, ConfigError> {
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(raw) => Ok(raw),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn write_in(
    dir: &Path,
    edit: impl FnOnce(&str) -> Result<String, ConfigError>,
) -> Result<(), ConfigError> {
    let updated = edit(&read_in(dir)?)?;
    std::fs::create_dir_all(dir)?;
    write_atomic(&dir.join(FILE), &updated)?;
    let schema = dir.join(SCHEMA_FILE);
    if std::fs::read_to_string(&schema).ok().as_deref() != Some(SCHEMA) {
        write_atomic(&schema, SCHEMA)?;
    }
    Ok(())
}

/// Through `meeting_format::write_atomic`: a unique temp file, synced, then
/// renamed over the target and the folder synced, so neither a crash nor a
/// power cut leaves a half-written or empty config behind.
fn write_atomic(path: &Path, contents: &str) -> Result<(), ConfigError> {
    Ok(meeting_format::write_atomic(path, contents.as_bytes())?)
}

/// `raw` with its `agent` section set to `agent`.
pub fn with_agent(raw: &str, agent: &AgentConfig) -> Result<String, ConfigError> {
    let binary_path = match &agent.binary_path {
        None => CstInputValue::Null,
        Some(path) if path.as_os_str().is_empty() => CstInputValue::Null,
        Some(path) => path
            .to_str()
            .ok_or_else(|| {
                ConfigError::Invalid(format!("agent.binary_path {path:?} is not valid UTF-8"))
            })?
            .into(),
    };
    // `null`, not a missing key, so the file shows the key exists (A14).
    let model = match agent.model.as_deref() {
        Some(model) if !model.trim().is_empty() => model.into(),
        _ => CstInputValue::Null,
    };
    with_section(
        raw,
        "agent",
        vec![
            ("harness", agent.harness.as_str().into()),
            ("model", model),
            ("binary_path", binary_path),
            ("auto_run", agent.auto_run.into()),
            ("timeout_sec", agent.timeout_sec.into()),
        ],
    )
}

/// `raw` with its `tickets` section set to `tickets`.
pub fn with_tickets(raw: &str, tickets: &TicketsConfig) -> Result<String, ConfigError> {
    with_section(
        raw,
        "tickets",
        vec![
            ("tracker", tickets.tracker.as_str().into()),
            ("tracker_mcp", tickets.tracker_mcp.as_str().into()),
        ],
    )
}

pub(super) fn with_section(
    raw: &str,
    section: &str,
    fields: Vec<(&str, CstInputValue)>,
) -> Result<String, ConfigError> {
    // A file that does not parse is refused, never overwritten: it is the
    // user's file, and a typo is no reason to lose the rest of it.
    let root = CstRootNode::parse(raw, &Default::default())
        .map_err(|error| ConfigError::Invalid(error.to_string()))?;
    let top = match root.value() {
        // A new or empty file: start it with the schema line.
        None => {
            let top = root.object_value_or_set();
            top.append("$schema", format!("./{SCHEMA_FILE}").into());
            top
        }
        Some(_) => root
            .object_value()
            .ok_or_else(|| ConfigError::Invalid("the top level must be an object".into()))?,
    };
    // A section that is there but not an object (`"agent": null`, `"agent":
    // "x"`) reads as `Invalid`; saving from the Setup screen replaces it with
    // a proper object, which is how the user fixes it.
    let object = top.object_value_or_set(section);
    for (name, value) in fields {
        match object.get(name) {
            Some(prop) => prop.set_value(value),
            None => {
                object.append(name, value);
            }
        }
    }
    Ok(root.to_string())
}
