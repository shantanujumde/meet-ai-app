//! Codex (`codex exec`) as a [`Harness`] (SPEC A11).
//!
//! Both runs go through `codex exec`, with the prompt on stdin (the trailing
//! `-`), in a fresh, empty working folder and a read-only sandbox:
//!
//! | Run | What it gets |
//! |---|---|
//! | Notes | `--ignore-user-config --ignore-rules`: no MCP servers, no hooks, no rules. Codex still reads its sign-in from `CODEX_HOME` |
//! | Sync | The user's `~/.codex/config.toml`, so their tracker's MCP server is there, narrowed to the tools in [`Job::allowed_tools`] |
//!
//! Codex takes the output schema only as a file (`--output-schema`) and
//! writes its reply only to a file (`-o`), so each run also gets a second
//! temp folder for those two files. It sits next to the working folder, not
//! inside it, so the folder the agent sees stays empty. Both folders are
//! deleted when the run ends, however it ends. What Codex prints on stdout is
//! progress, and is ignored.
//!
//! How `exec` handles approval for MCP tool calls is recorded in SPEC A11 and
//! `docs/manual-checks/worktree-b-tur5.md`. No code here depends on it.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::{AgentError, Harness, Install, Job, JobKind, OutputCheck, parse_json, process};

/// The harness id, as written to `agent.harness` and `analyzed_by`.
const ID: &str = "codex";

/// The CLI's name as the user knows it, for errors such as "Codex is not
/// installed".
const DISPLAY_NAME: &str = "Codex";

/// What runs when the config sets no `agent.binary_path`: `codex` from `PATH`.
const DEFAULT_PROGRAM: &str = "codex";

/// The schema file Codex is pointed at with `--output-schema`.
const SCHEMA_FILE: &str = "schema.json";

/// The file Codex writes its last message to (`-o`).
const REPLY_FILE: &str = "reply.json";

/// Largest reply file read. Same limit as the stdout of the other CLIs.
const MAX_REPLY_BYTES: u64 = 8 * 1024 * 1024;

/// Time limit for `codex debug models`, which makes no model call.
const MODELS_TIMEOUT: Duration = Duration::from_secs(15);

/// Runs the user's Codex CLI.
#[derive(Debug, Clone, Default)]
pub struct CodexHarness {
    /// `agent.binary_path` from the config. `None` runs `codex` from `PATH`.
    binary_path: Option<PathBuf>,
}

impl CodexHarness {
    /// Runs `codex` from `PATH`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs the binary at `path` (the config's `agent.binary_path`).
    pub fn with_binary(path: impl Into<PathBuf>) -> Self {
        Self {
            binary_path: Some(path.into()),
        }
    }

    fn program(&self) -> &OsStr {
        self.binary_path
            .as_deref()
            .map_or(OsStr::new(DEFAULT_PROGRAM), Path::as_os_str)
    }

    /// Asks Codex for its model list, with the same time limit, kill and
    /// clean-up as a real run.
    fn list_models(&self) -> Result<Vec<String>, AgentError> {
        let mut job = Job::notes(String::new(), serde_json::json!({}));
        job.timeout = MODELS_TIMEOUT;
        let work = process::fresh_work_dir(&job)?;
        let mut command = Command::new(self.program());
        command.args(["debug", "models"]);
        let out = process::run_cli(DISPLAY_NAME, command, &job, work.path())?;
        Ok(listed_models(&out.stdout))
    }
}

impl Harness for CodexHarness {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self) -> Option<Install> {
        // Finding Codex (a login-shell `PATH` lookup, `--version`, sign-in) is
        // TUR-6's `crate::detect::codex`, which replaces this body. Nothing
        // calls this yet.
        None
    }

    /// Codex's own list, from `codex debug models`, in Codex's order.
    ///
    /// Empty when Codex cannot be asked; the setup picker then offers only
    /// "Codex's default". When [`Job::model`] is `None`, Codex picks its own
    /// default: on 2026-10-01 that was `gpt-5.6-terra` (Codex 0.152.1).
    fn models(&self) -> Vec<String> {
        self.list_models().unwrap_or_else(|e| {
            tracing::debug!("could not list Codex's models: {e}");
            Vec::new()
        })
    }

    fn run(&self, job: &Job) -> Result<serde_json::Value, AgentError> {
        let check = OutputCheck::new(&job.schema)?;
        let work = process::fresh_work_dir(job)?;
        let io = reply_dir(job)?;
        let schema = io.path().join(SCHEMA_FILE);
        let reply = io.path().join(REPLY_FILE);

        let mut command = Command::new(self.program());
        command.args(exec_args(job, &schema, &reply));
        // Stdout is Codex's progress; the reply is the `-o` file.
        process::run_cli(DISPLAY_NAME, command, job, work.path())?;

        let text = read_reply(&reply)?;
        check.check(parse_json(&text)?)
    }
}

/// Makes the run's folder for the schema and reply files, next to its working
/// folder, and writes the schema into it. Deleted when dropped.
fn reply_dir(job: &Job) -> Result<tempfile::TempDir, AgentError> {
    let root = job
        .work_root
        .canonicalize()
        .map_err(|e| could_not_start(format!("could not use the working folder: {e}")))?;
    let dir = tempfile::Builder::new()
        .prefix("meet-ai-codex-io-")
        .tempdir_in(&root)
        .map_err(|e| could_not_start(format!("could not make a folder for the reply: {e}")))?;
    let schema = serde_json::to_vec(&job.schema)
        .map_err(|e| could_not_start(format!("could not write the output schema: {e}")))?;
    std::fs::write(dir.path().join(SCHEMA_FILE), schema)
        .map_err(|e| could_not_start(format!("could not write the output schema: {e}")))?;
    Ok(dir)
}

fn could_not_start(reason: impl Into<String>) -> AgentError {
    AgentError::CouldNotStart {
        reason: reason.into(),
    }
}

/// The arguments for `codex exec`.
///
/// The prompt is never one of them (arguments show up in `ps`); the trailing
/// `-` makes Codex read it from stdin, which [`process::run_cli`] writes.
fn exec_args(job: &Job, schema: &Path, reply: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["exec", "--ephemeral", "--skip-git-repo-check"]
        .map(OsString::from)
        .into();
    if job.kind == JobKind::Notes {
        // No user config means no MCP servers, plugins or hooks; no rules
        // means none of the user's execpolicy `.rules` files.
        args.extend(["--ignore-user-config", "--ignore-rules"].map(OsString::from));
    }
    args.extend(["-s", "read-only"].map(OsString::from));
    if job.kind == JobKind::Sync {
        for setting in enabled_tools_settings(&job.allowed_tools) {
            args.push("-c".into());
            args.push(setting.into());
        }
    }
    if let Some(model) = job.model.as_deref().filter(|m| !m.trim().is_empty()) {
        args.push("--model".into());
        args.push(model.into());
    }
    args.push("--output-schema".into());
    args.push(schema.into());
    args.push("-o".into());
    args.push(reply.into());
    args.push("-".into());
    args
}

/// One `mcp_servers.<server>.enabled_tools=[...]` setting per MCP server
/// named in `allowed_tools`, in server-name order.
///
/// `allowed_tools` uses Claude Code's names, `mcp__<server>__<tool>`; anything
/// else is skipped. This only narrows the servers it names. Any other MCP
/// server in the user's Codex config stays on with all its tools, which is a
/// limit of this run compared to Claude Code's `--allowedTools`.
fn enabled_tools_settings(allowed_tools: &[String]) -> Vec<String> {
    let mut by_server: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for name in allowed_tools {
        let Some((server, tool)) = mcp_tool(name) else {
            tracing::debug!(tool = %name, "not an MCP tool name; Codex is not told about it");
            continue;
        };
        let tools = by_server.entry(server).or_default();
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    by_server
        .into_iter()
        .map(|(server, tools)| {
            let list: Vec<String> = tools.into_iter().map(toml_string).collect();
            format!(
                "mcp_servers.{}.enabled_tools=[{}]",
                toml_key(server),
                list.join(",")
            )
        })
        .collect()
}

/// Splits `mcp__<server>__<tool>` into its server and tool. The server ends
/// at the first `__` after the prefix.
fn mcp_tool(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// `key` as one segment of a TOML dotted key: bare when TOML allows it,
/// quoted otherwise (a `.` in it would split it into two segments).
fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if bare {
        key.to_owned()
    } else {
        toml_string(key)
    }
}

/// `text` as a TOML basic string, `"..."`.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Reads the reply file Codex wrote. The text is never put in an error.
fn read_reply(path: &Path) -> Result<String, AgentError> {
    let invalid = |reason: String| AgentError::InvalidJson { reason };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(invalid("Codex finished without writing a reply".to_owned()));
        }
        Err(e) => return Err(invalid(format!("could not read Codex's reply: {e}"))),
    };
    let too_big = || {
        invalid(format!(
            "the reply was larger than {} MiB",
            MAX_REPLY_BYTES / (1024 * 1024)
        ))
    };
    let size = file
        .metadata()
        .map_err(|e| invalid(format!("could not read Codex's reply: {e}")))?
        .len();
    if size > MAX_REPLY_BYTES {
        return Err(too_big());
    }
    // The file could still grow after the size check; never read past the cap.
    let mut bytes = Vec::new();
    file.take(MAX_REPLY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(format!("could not read Codex's reply: {e}")))?;
    if bytes.len() as u64 > MAX_REPLY_BYTES {
        return Err(too_big());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// What `codex debug models` prints: `{"models":[...]}`.
#[derive(serde::Deserialize)]
struct ModelList {
    models: Vec<ModelEntry>,
}

/// One model in that list. Other fields are ignored.
#[derive(serde::Deserialize)]
struct ModelEntry {
    slug: String,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    priority: Option<i64>,
}

/// The models `codex debug models` lists for people to pick (`"visibility":
/// "list"`), by `priority` from low to high, those without one last. Hidden
/// models are left out, and so is each repeat of a slug. Empty when `json` is
/// not that shape.
fn listed_models(json: &str) -> Vec<String> {
    let list: ModelList = match serde_json::from_str(json.trim()) {
        Ok(list) => list,
        Err(e) => {
            tracing::debug!("Codex's model list was not the expected JSON: {e}");
            return Vec::new();
        }
    };
    let mut listed: Vec<ModelEntry> = list
        .models
        .into_iter()
        .filter(|m| m.visibility.as_deref() == Some("list"))
        .collect();
    listed.sort_by_key(|m| (m.priority.is_none(), m.priority));
    let mut slugs: Vec<String> = Vec::with_capacity(listed.len());
    for model in listed {
        if !slugs.contains(&model.slug) {
            slugs.push(model.slug);
        }
    }
    slugs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args_of(job: &Job) -> Vec<String> {
        exec_args(
            job,
            Path::new("/io/schema.json"),
            Path::new("/io/reply.json"),
        )
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect()
    }

    #[test]
    fn a_notes_run_gets_no_user_config_and_no_rules() {
        let job = Job::notes("SECRET TRANSCRIPT", json!({}));
        assert_eq!(
            args_of(&job),
            [
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "--ignore-user-config",
                "--ignore-rules",
                "-s",
                "read-only",
                "--output-schema",
                "/io/schema.json",
                "-o",
                "/io/reply.json",
                "-",
            ]
        );
    }

    #[test]
    fn the_model_is_passed_only_when_one_is_set() {
        let mut job = Job::notes("", json!({}));
        job.model = Some("gpt-5.6-terra".into());
        let args = args_of(&job);
        let at = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[at + 1], "gpt-5.6-terra");
        assert_eq!(args[at - 1], "read-only");

        for model in [None, Some(String::new()), Some("  ".into())] {
            job.model = model;
            assert!(!args_of(&job).contains(&"--model".to_owned()), "{job:?}");
        }
    }

    #[test]
    fn a_sync_run_loads_the_user_config_and_narrows_the_tracker() {
        let mut job = Job::sync(
            "task",
            json!({}),
            vec![
                "mcp__linear__create_issue".into(),
                "mcp__linear__get_issue".into(),
            ],
        );
        job.model = Some("gpt-5.5".into());
        assert_eq!(
            args_of(&job),
            [
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "-s",
                "read-only",
                "-c",
                "mcp_servers.linear.enabled_tools=[\"create_issue\",\"get_issue\"]",
                "--model",
                "gpt-5.5",
                "--output-schema",
                "/io/schema.json",
                "-o",
                "/io/reply.json",
                "-",
            ]
        );
    }

    #[test]
    fn a_notes_run_ignores_allowed_tools() {
        let mut job = Job::notes("", json!({}));
        job.allowed_tools = vec!["mcp__linear__create_issue".into()];
        assert!(!args_of(&job).contains(&"-c".to_owned()));
    }

    #[test]
    fn allowed_tools_are_grouped_by_server_and_odd_names_skipped() {
        let tools: Vec<String> = [
            "mcp__linear__create_issue",
            "Bash",
            "mcp__github__create_issue",
            "mcp__linear__create_issue",
            "mcp____nothing",
            "mcp__no_tool__",
            "mcp__linear__update_issue",
        ]
        .map(String::from)
        .into();
        assert_eq!(
            enabled_tools_settings(&tools),
            [
                "mcp_servers.github.enabled_tools=[\"create_issue\"]",
                "mcp_servers.linear.enabled_tools=[\"create_issue\",\"update_issue\"]",
            ]
        );
        assert!(enabled_tools_settings(&[]).is_empty());
    }

    #[test]
    fn server_names_toml_cannot_take_bare_are_quoted() {
        let tools: Vec<String> = [
            "mcp__my.server__create_issue",
            "mcp__claude ai Linear__save_issue",
            "mcp__ok-name_1__a\"b\\c",
        ]
        .map(String::from)
        .into();
        assert_eq!(
            enabled_tools_settings(&tools),
            [
                "mcp_servers.\"claude ai Linear\".enabled_tools=[\"save_issue\"]",
                "mcp_servers.\"my.server\".enabled_tools=[\"create_issue\"]",
                "mcp_servers.ok-name_1.enabled_tools=[\"a\\\"b\\\\c\"]",
            ]
        );
    }

    #[test]
    fn toml_strings_escape_control_characters() {
        assert_eq!(toml_string("a\nb\u{7}"), "\"a\\u000Ab\\u0007\"");
        assert_eq!(toml_key(""), "\"\"");
    }

    #[test]
    fn the_model_list_keeps_listed_slugs_in_priority_order() {
        let json = r#"{"models":[{"slug":"gpt-5.6-sol","visibility":"list","priority":1},{"slug":"gpt-5.6-terra","visibility":"list","priority":2},{"slug":"gpt-daybreak-blue-latest","visibility":"hide","priority":3},{"slug":"gpt-5.5","visibility":"list","priority":7}]}"#;
        assert_eq!(
            listed_models(json),
            ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.5"]
        );
    }

    #[test]
    fn the_model_list_sorts_missing_priorities_last_and_drops_repeats() {
        let json = json!({ "models": [
            { "slug": "no-priority", "visibility": "list" },
            { "slug": "b", "visibility": "list", "priority": 5, "extra": true },
            { "slug": "a", "visibility": "list", "priority": 5 },
            { "slug": "first", "visibility": "list", "priority": -1 },
            { "slug": "b", "visibility": "list", "priority": 9 },
            { "slug": "no-visibility", "priority": 0 },
        ]})
        .to_string();
        assert_eq!(listed_models(&json), ["first", "b", "a", "no-priority"]);
    }

    #[test]
    fn a_model_list_that_is_not_the_expected_json_is_empty() {
        assert!(listed_models("").is_empty());
        assert!(listed_models("Error: not signed in").is_empty());
        assert!(listed_models(r#"{"data":[]}"#).is_empty());
    }

    #[test]
    fn the_binary_path_overrides_codex_from_path() {
        assert_eq!(CodexHarness::new().program(), OsStr::new("codex"));
        assert_eq!(
            CodexHarness::with_binary("/opt/codex/bin/codex").program(),
            OsStr::new("/opt/codex/bin/codex")
        );
        assert_eq!(CodexHarness::new().id(), "codex");
        assert!(CodexHarness::new().detect().is_none());
    }

    #[test]
    fn a_missing_reply_file_is_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_reply(&dir.path().join(REPLY_FILE)).unwrap_err();
        match err {
            AgentError::InvalidJson { reason } => assert!(reason.contains("Codex"), "{reason}"),
            other => panic!("expected InvalidJson, got {other:?}"),
        }
    }

    #[test]
    fn a_reply_file_over_the_limit_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(REPLY_FILE);
        let file = File::create(&path).unwrap();
        file.set_len(MAX_REPLY_BYTES + 1).unwrap();
        match read_reply(&path).unwrap_err() {
            AgentError::InvalidJson { reason } => assert!(reason.contains("8 MiB"), "{reason}"),
            other => panic!("expected InvalidJson, got {other:?}"),
        }
    }
}
