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

// TUR-9 (Setup screens) adds the IPC commands that call into this module.
#![allow(dead_code)]

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstRootNode};

use super::FILE;
use super::agent_section::{AgentConfig, ConfigError, TicketsConfig, parse_agent, parse_tickets};

/// The JSON Schema for `config.jsonc`, kept in the repo and shipped inside the
/// binary.
pub const SCHEMA: &str = include_str!("../../config.schema.json");
const SCHEMA_FILE: &str = "config.schema.json";

/// `agent` from `~/Meetings/.app/config.jsonc`. A missing file or section is
/// the SPEC §3.5 defaults; an unknown `harness` is an error.
pub fn agent() -> Result<AgentConfig, ConfigError> {
    parse_agent(&read_in(&app_dir()?)?)
}

/// `tickets` from `~/Meetings/.app/config.jsonc`, defaults when missing.
pub fn tickets() -> Result<TicketsConfig, ConfigError> {
    parse_tickets(&read_in(&app_dir()?)?)
}

/// Save `agent` into `~/Meetings/.app/config.jsonc`, keeping everything else.
pub fn set_agent(agent: &AgentConfig) -> Result<(), ConfigError> {
    write_in(&app_dir()?, |raw| with_agent(raw, agent))
}

/// Save `tickets` into `~/Meetings/.app/config.jsonc`, keeping everything else.
pub fn set_tickets(tickets: &TicketsConfig) -> Result<(), ConfigError> {
    write_in(&app_dir()?, |raw| with_tickets(raw, tickets))
}

fn app_dir() -> Result<PathBuf, ConfigError> {
    let root = crate::meetings::root()
        .map_err(|error| ConfigError::Io(std::io::Error::other(error.message)))?;
    Ok(meeting_format::layout::app_dir(&root))
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

/// Write to a sibling temp file, then rename over the target, so a crash
/// mid-write never leaves a half-written config behind.
fn write_atomic(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    std::fs::write(&temp, contents)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// `raw` with its `agent` section set to `agent`.
pub fn with_agent(raw: &str, agent: &AgentConfig) -> Result<String, ConfigError> {
    let binary_path = match &agent.binary_path {
        None => CstInputValue::Null,
        Some(path) => path
            .to_str()
            .ok_or_else(|| {
                ConfigError::Invalid(format!("agent.binary_path {path:?} is not valid UTF-8"))
            })?
            .into(),
    };
    with_section(
        raw,
        "agent",
        vec![
            ("harness", agent.harness.as_str().into()),
            ("model", agent.model.as_str().into()),
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

fn with_section(
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
