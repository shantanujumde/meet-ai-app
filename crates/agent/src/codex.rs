//! Codex (`codex exec`) as a [`Harness`] (SPEC A11).
//!
//! Both runs go through `codex exec`, with the prompt on stdin (the trailing
//! `-`), in a fresh, empty working folder and a read-only sandbox:
//!
//! | Run | What it gets |
//! |---|---|
//! | Notes | `--ignore-user-config --ignore-rules`: no MCP servers, no hooks, no rules. Codex still reads its sign-in from `CODEX_HOME` |
//! | Sync | The user's `~/.codex/config.toml`, so their tracker's MCP server is there, with only the tools in [`Job::allowed_tools`] pre-approved. Every other MCP server is turned off, and so are ChatGPT connectors ("apps") and plugins |
//!
//! Codex takes the output schema only as a file (`--output-schema`) and
//! writes its reply only to a file (`-o`), so each run also gets a second
//! temp folder for those two files. It sits next to the working folder, not
//! inside it, so the folder the agent sees stays empty. Both folders are
//! deleted when the run ends, however it ends. What Codex prints on stdout is
//! progress, and is ignored.
//!
//! `exec` runs with `approval: never`, so an MCP tool call that is not
//! pre-approved is refused, and Codex still exits 0. A sync run therefore
//! pre-approves the tracker's tools in config ([`servers_setting`]). What was
//! measured is in SPEC A11 and `docs/manual-checks/worktree-e-tur16.md`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::mcp::{self, McpServer, McpStatus};
use crate::process::{self, MAX_STDOUT_BYTES, could_not_start, reply_too_big};
use crate::{AgentError, Harness, Install, Job, JobKind, OutputCheck, parse_json};

/// The harness id, as written to `agent.harness` and `analyzed_by`.
pub const ID: &str = "codex";

/// The CLI's name as the user knows it, for errors such as "Codex is not
/// installed".
const DISPLAY_NAME: &str = "Codex";

/// What runs when the config sets no `agent.binary_path`: `codex` from `PATH`.
const DEFAULT_PROGRAM: &str = "codex";

/// The schema file Codex is pointed at with `--output-schema`.
const SCHEMA_FILE: &str = "schema.json";

/// The file Codex writes its last message to (`-o`).
const REPLY_FILE: &str = "reply.json";

/// Largest reply file read: the same limit as a reply on stdout.
const MAX_REPLY_BYTES: u64 = MAX_STDOUT_BYTES as u64;

/// Time limit for `codex debug models`, which makes no model call.
const MODELS_TIMEOUT: Duration = Duration::from_secs(15);

/// Codex features a sync run turns off (`--disable <feature>`): ChatGPT
/// connectors ("apps") and plugins, which bring tools of their own that are
/// not in the MCP server list, and Codex's built-in tools (shell, image,
/// sub-agents, browser, computer use). A tracker reachable only through a
/// plugin is therefore "not synced". `unified_exec` stays on even with
/// `--disable`, so it is not listed.
const SYNC_FEATURES_OFF: [&str; 8] = [
    "apps",
    "plugins",
    "shell_tool",
    "image_generation",
    "view_image",
    "multi_agent",
    "browser_use",
    "computer_use",
];

/// The `-c` setting that keeps the skills list out of a sync run's prompt.
const SKILLS_OFF_SETTING: &str = "skills.include_instructions=false";

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

    /// Asks Codex for its model list, with the same time limit, kill and
    /// clean-up as a real run.
    fn list_models(&self) -> Result<Vec<String>, AgentError> {
        let mut command = self.command();
        command.args(["debug", "models"]);
        let out = process::run_probe(DISPLAY_NAME, command, MODELS_TIMEOUT, &std::env::temp_dir())?;
        Ok(listed_models(&out.stdout))
    }

    /// The MCP servers Codex will load for a sync run, from `codex mcp list
    /// --json` with the same features off as the run itself. Same work root,
    /// time limit (at most [`mcp::LIST_TIMEOUT`]) and Cancel as `job`.
    fn configured_servers(&self, job: &Job) -> Result<Vec<McpServer>, AgentError> {
        let mut command = self.command();
        command
            .args(disabled_features())
            .args(["mcp", "list", "--json"]);
        let mut list = Job::notes(String::new(), serde_json::Value::Null);
        list.timeout = job.timeout.min(mcp::LIST_TIMEOUT);
        list.work_root = job.work_root.clone();
        list.cancel = job.cancel.clone();
        let dir = process::fresh_work_dir(&list)?;
        let out = process::run_cli(DISPLAY_NAME, command, &list, dir.path())?;
        mcp::parse_codex_list(&out.stdout)
    }

    /// The arguments a sync run adds: the features it turns off, and the one
    /// `mcp_servers` setting from [`servers_setting`].
    fn sync_args(&self, job: &Job) -> Result<Vec<OsString>, AgentError> {
        let configured = self.configured_servers(job)?;
        let mut args = disabled_features();
        if let Some(setting) = servers_setting(&job.allowed_tools, &configured)? {
            args.push("-c".into());
            args.push(setting.into());
        }
        args.push("-c".into());
        args.push(SKILLS_OFF_SETTING.into());
        Ok(args)
    }

    /// A `Command` for Codex. A configured binary gets its own folder first
    /// on `PATH` ([`process::cli_command`]), since an app opened from Finder
    /// has a short one.
    fn command(&self) -> Command {
        match &self.binary_path {
            Some(path) => process::cli_command(path),
            None => Command::new(DEFAULT_PROGRAM),
        }
    }
}

impl Harness for CodexHarness {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self) -> Option<Install> {
        crate::detect::codex(self.binary_path.as_deref())
    }

    /// Codex's own list, from `codex debug models`, in Codex's order.
    ///
    /// Starts Codex and blocks until it answers, for up to 15 s (it makes no
    /// model call); call it off the UI thread. Empty when Codex cannot be
    /// asked; the setup picker then offers Codex's entries in `models.json`
    /// ([`crate::models`]). When [`Job::model`] is `None`, Codex picks its own
    /// default: on 2026-10-01 that was `gpt-5.6-terra` (Codex 0.152.1).
    fn models(&self) -> Vec<String> {
        self.list_models().unwrap_or_else(|e| {
            tracing::debug!("could not list Codex's models: {e}");
            Vec::new()
        })
    }

    fn run(&self, job: &Job) -> Result<serde_json::Value, AgentError> {
        let check = OutputCheck::new(&job.schema)?;
        let sync_args = match job.kind {
            JobKind::Sync => self.sync_args(job)?,
            JobKind::Notes => Vec::new(),
        };
        let work = process::fresh_work_dir(job)?;
        let io = reply_dir(job, work.path())?;
        let schema = io.path().join(SCHEMA_FILE);
        let reply = io.path().join(REPLY_FILE);

        let mut command = self.command();
        command.args(exec_args(job, &sync_args, &schema, &reply));
        // Stdout is Codex's progress; the reply is the `-o` file.
        process::run_cli(DISPLAY_NAME, command, job, work.path()).map_err(|e| match e {
            AgentError::CliFailed { status, stderr } => AgentError::CliFailed {
                status,
                stderr: without_prompt(&stderr, &job.prompt),
            },
            other => other,
        })?;

        let text = read_reply(&reply)?;
        check.check(parse_json(&text)?)
    }
}

/// Makes the run's folder for the schema and reply files, next to its working
/// folder `work`, and writes the schema into it. Deleted when dropped.
fn reply_dir(job: &Job, work: &Path) -> Result<tempfile::TempDir, AgentError> {
    let root = work
        .parent()
        .ok_or_else(|| could_not_start("the working folder has no parent folder"))?;
    let dir = tempfile::Builder::new()
        .prefix("meet-ai-codex-io-")
        .tempdir_in(root)
        .map_err(|e| could_not_start(format!("could not make a folder for the reply: {e}")))?;
    let schema = serde_json::to_vec(&job.schema)
        .map_err(|e| could_not_start(format!("could not write the output schema: {e}")))?;
    std::fs::write(dir.path().join(SCHEMA_FILE), schema)
        .map_err(|e| could_not_start(format!("could not write the output schema: {e}")))?;
    Ok(dir)
}

/// The arguments for `codex exec`. `sync_args` is what
/// [`CodexHarness::sync_args`] gave; a notes run ignores it.
///
/// The prompt is never one of them (arguments show up in `ps`); the trailing
/// `-` makes Codex read it from stdin, which [`process::run_cli`] writes.
fn exec_args(job: &Job, sync_args: &[OsString], schema: &Path, reply: &Path) -> Vec<OsString> {
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
        args.extend(sync_args.iter().cloned());
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

/// `--disable <feature>` for each of [`SYNC_FEATURES_OFF`].
fn disabled_features() -> Vec<OsString> {
    SYNC_FEATURES_OFF
        .iter()
        .flat_map(|feature| ["--disable", feature])
        .map(OsString::from)
        .collect()
}

/// The one `mcp_servers={...}` setting of a sync run, or `None` when there is
/// nothing to set.
///
/// `allowed_tools` uses Claude Code's names, `mcp__<server>__<tool>`; anything
/// else is skipped. `configured` is every server Codex will load
/// ([`CodexHarness::configured_servers`]). In the setting:
///
/// - each server named in `allowed_tools` gets its tools pre-approved, since
///   `exec` refuses any MCP call that is not. A tool of `*`
///   (`mcp__linear__*`, what [`crate::mcp::tracker_tools`] gives) means all
///   of that server's tools: `default_tools_approval_mode="approve"`, even
///   when other tools of it are listed too. Otherwise only the listed tools
///   are on (`enabled_tools`), each with `approval_mode="approve"`;
/// - every other configured server that is on gets `enabled=false`.
///
/// It is one inline table with every name a quoted TOML key, because Codex
/// 0.152.1 splits a dotted `-c` key on every `.`, even inside quotes, while an
/// inline table merges into the user's entry whatever its name. A second
/// `-c mcp_servers={...}` would replace the first, so there is only one.
/// Turning off a server Codex does not have fails the whole run, which is why
/// only listed servers are turned off.
///
/// A server named in `allowed_tools` is refused ([`check_tracker`]) when
/// Codex does not list it, lists it as off, or its name has a `.`.
fn servers_setting(
    allowed_tools: &[String],
    configured: &[McpServer],
) -> Result<Option<String>, AgentError> {
    // `None` means all of that server's tools.
    let mut allowed: BTreeMap<&str, Option<Vec<&str>>> = BTreeMap::new();
    for name in allowed_tools {
        let Some((server, tool)) = mcp_tool(name) else {
            tracing::debug!(tool = %name, "not an MCP tool name; Codex is not told about it");
            continue;
        };
        let tools = allowed.entry(server).or_insert_with(|| Some(Vec::new()));
        if tool == "*" {
            *tools = None;
        } else if let Some(list) = tools
            && !list.contains(&tool)
        {
            list.push(tool);
        }
    }

    let mut entries: Vec<String> = Vec::new();
    for (server, tools) in &allowed {
        check_tracker(server, configured)?;
        let entry = match tools {
            None => r#"default_tools_approval_mode="approve""#.to_owned(),
            Some(tools) => {
                let names: Vec<String> = tools.iter().map(|t| toml_string(t)).collect();
                let approvals: Vec<String> = names
                    .iter()
                    .map(|t| format!(r#"{t}={{approval_mode="approve"}}"#))
                    .collect();
                format!(
                    "enabled_tools=[{}],tools={{{}}}",
                    names.join(","),
                    approvals.join(",")
                )
            }
        };
        entries.push(format!("{}={{{entry}}}", toml_string(server)));
    }
    for server in configured {
        if server.status != McpStatus::Disabled && !allowed.contains_key(server.name.as_str()) {
            entries.push(format!("{}={{enabled=false}}", toml_string(&server.name)));
        }
    }
    Ok((!entries.is_empty()).then(|| format!("mcp_servers={{{}}}", entries.join(","))))
}

/// Refuses a tracker server a sync run cannot use: one Codex does not list,
/// one turned off in Codex's config, and one whose name has a `.`, since
/// Codex 0.152.1 loads such a server but never shows its tools to the model.
fn check_tracker(server: &str, configured: &[McpServer]) -> Result<(), AgentError> {
    if server.contains('.') {
        return Err(could_not_start(
            "Codex can't use a tracker server whose name contains a dot. Rename it in ~/.codex/config.toml.",
        ));
    }
    match configured.iter().find(|s| s.name == server) {
        None => Err(could_not_start(format!(
            "Codex has no MCP server named \"{server}\". Pick the tracker again in Settings."
        ))),
        Some(s) if s.status == McpStatus::Disabled => Err(could_not_start(format!(
            "The MCP server \"{server}\" is turned off in Codex. Turn it on in ~/.codex/config.toml."
        ))),
        Some(_) => Ok(()),
    }
}

/// Splits `mcp__<server>__<tool>` into its server and tool. The server ends
/// at the first `__` after the prefix.
fn mcp_tool(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// `stderr` without the lines that are also in `prompt`.
///
/// Codex's start-up banner on stderr repeats the whole prompt, so a failed
/// run's stderr can hold transcript lines, and that text is shown in the
/// meeting view. A line cut short by the stderr cap is part of the prompt
/// too, so it goes as well. What is left is Codex's own output.
fn without_prompt(stderr: &str, prompt: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.is_empty() || !prompt.contains(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
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
    let size = file
        .metadata()
        .map_err(|e| invalid(format!("could not read Codex's reply: {e}")))?
        .len();
    if size > MAX_REPLY_BYTES {
        return Err(reply_too_big());
    }
    // The file could still grow after the size check; never read past the cap.
    let mut bytes = Vec::new();
    file.take(MAX_REPLY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(format!("could not read Codex's reply: {e}")))?;
    if bytes.len() as u64 > MAX_REPLY_BYTES {
        return Err(reply_too_big());
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
        args_with(job, &[])
    }

    fn args_with(job: &Job, sync_args: &[&str]) -> Vec<String> {
        let sync_args: Vec<OsString> = sync_args.iter().map(OsString::from).collect();
        exec_args(
            job,
            &sync_args,
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

    fn server(name: &str, status: McpStatus) -> McpServer {
        McpServer {
            name: name.to_owned(),
            status,
        }
    }

    fn tools(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn a_sync_run_loads_the_user_config_and_adds_its_own_settings() {
        let mut job = Job::sync("task", json!({}), tools(&["mcp__linear__*"]));
        job.model = Some("gpt-5.5".into());
        assert_eq!(
            args_with(&job, &["--disable", "apps", "-c", "mcp_servers={}"]),
            [
                "exec",
                "--ephemeral",
                "--skip-git-repo-check",
                "-s",
                "read-only",
                "--disable",
                "apps",
                "-c",
                "mcp_servers={}",
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
    fn a_notes_run_ignores_sync_settings() {
        let mut job = Job::notes("", json!({}));
        job.allowed_tools = tools(&["mcp__linear__create_issue"]);
        let args = args_with(&job, &["--disable", "apps", "-c", "mcp_servers={}"]);
        assert!(!args.contains(&"-c".to_owned()), "{args:?}");
        assert!(!args.contains(&"--disable".to_owned()), "{args:?}");
    }

    #[test]
    fn a_sync_run_turns_off_connectors_plugins_and_built_in_tools() {
        let args = disabled_features();
        let expected: Vec<std::ffi::OsString> = [
            "apps",
            "plugins",
            "shell_tool",
            "image_generation",
            "view_image",
            "multi_agent",
            "browser_use",
            "computer_use",
        ]
        .iter()
        .flat_map(|f| ["--disable".into(), (*f).into()])
        .collect();
        assert_eq!(args, expected);
        assert!(!args.iter().any(|a| a == "unified_exec"));
    }

    #[test]
    fn a_star_tool_pre_approves_the_whole_tracker_and_turns_the_rest_off() {
        let configured = [
            server("linear", McpStatus::Configured),
            server("node_repl", McpStatus::Configured),
            server("needs-login", McpStatus::NeedsAuth),
            server("already-off", McpStatus::Disabled),
        ];
        assert_eq!(
            servers_setting(&tools(&["mcp__linear__*"]), &configured)
                .unwrap()
                .unwrap(),
            r#"mcp_servers={"linear"={default_tools_approval_mode="approve"},"node_repl"={enabled=false},"needs-login"={enabled=false}}"#
        );
        // `*` wins over listed tools of the same server, in either order.
        for list in [
            ["mcp__linear__create_issue", "mcp__linear__*"],
            ["mcp__linear__*", "mcp__linear__create_issue"],
        ] {
            assert_eq!(
                servers_setting(&tools(&list), &configured[..1])
                    .unwrap()
                    .unwrap(),
                r#"mcp_servers={"linear"={default_tools_approval_mode="approve"}}"#
            );
        }
    }

    #[test]
    fn listed_tools_are_the_only_ones_on_and_each_is_pre_approved() {
        let configured = [
            server("github", McpStatus::Configured),
            server("linear", McpStatus::Configured),
        ];
        let allowed = tools(&[
            "mcp__linear__create_issue",
            "Bash",
            "mcp__github__create_issue",
            "mcp__linear__create_issue",
            "mcp____nothing",
            "mcp__no_tool__",
            "mcp__linear__update_issue",
        ]);
        assert_eq!(
            servers_setting(&allowed, &configured).unwrap().unwrap(),
            concat!(
                r#"mcp_servers={"github"={enabled_tools=["create_issue"],tools={"create_issue"={approval_mode="approve"}}},"#,
                r#""linear"={enabled_tools=["create_issue","update_issue"],tools={"create_issue"={approval_mode="approve"},"update_issue"={approval_mode="approve"}}}}"#
            )
        );
    }

    #[test]
    fn with_no_mcp_tools_every_server_is_turned_off() {
        let configured = [server("node_repl", McpStatus::Configured)];
        assert_eq!(
            servers_setting(&tools(&["Bash"]), &configured)
                .unwrap()
                .unwrap(),
            r#"mcp_servers={"node_repl"={enabled=false}}"#
        );
        assert_eq!(servers_setting(&[], &[]).unwrap(), None);
        let off = [server("off", McpStatus::Disabled)];
        assert_eq!(servers_setting(&[], &off).unwrap(), None);
    }

    #[test]
    fn any_other_server_name_is_turned_off_as_a_quoted_key() {
        let configured = [
            server("linear", McpStatus::Configured),
            server("my.server", McpStatus::Configured),
            server("a:b@c/d", McpStatus::Configured),
            server("sp ace", McpStatus::Configured),
            server("quo\"te\\", McpStatus::Configured),
        ];
        assert_eq!(
            servers_setting(&tools(&["mcp__linear__*"]), &configured)
                .unwrap()
                .unwrap(),
            concat!(
                r#"mcp_servers={"linear"={default_tools_approval_mode="approve"},"#,
                r#""my.server"={enabled=false},"a:b@c/d"={enabled=false},"#,
                r#""sp ace"={enabled=false},"quo\"te\\"={enabled=false}}"#
            )
        );
    }

    #[test]
    fn a_tracker_codex_cannot_use_is_refused_in_plain_words() {
        let configured = [
            server("my.linear", McpStatus::Configured),
            server("off", McpStatus::Disabled),
        ];
        for (tracker, expected) in [
            (
                "mcp__my.linear__*",
                "name contains a dot. Rename it in ~/.codex/config.toml.",
            ),
            (
                "mcp__missing__*",
                "Codex has no MCP server named \"missing\"",
            ),
            ("mcp__off__create_issue", "\"off\" is turned off in Codex"),
        ] {
            match servers_setting(&tools(&[tracker]), &configured).unwrap_err() {
                AgentError::CouldNotStart { reason } => {
                    assert!(reason.contains(expected), "{tracker}: {reason}");
                }
                other => panic!("{tracker}: expected CouldNotStart, got {other:?}"),
            }
        }
    }

    #[test]
    fn stderr_loses_the_prompt_codex_echoes_but_keeps_its_own_lines() {
        let prompt = "Summarize this.\n[00:00:05] Priya: layoffs on Friday\n[00:00:12] Sam: ok";
        let stderr = "OpenAI Codex v0.152.1\nuser\nSummarize this.\n[00:00:05] Priya: layoffs on Friday\n[00:00:12] Sam: ok\n\nERROR: you are not logged in";
        let left = without_prompt(stderr, prompt);
        assert!(!left.contains("layoffs"), "{left}");
        assert!(!left.contains("Sam"), "{left}");
        assert!(left.starts_with("OpenAI Codex"), "{left}");
        assert!(left.ends_with("ERROR: you are not logged in"), "{left}");

        // The stderr cap can cut a prompt line in half; that half goes too.
        assert_eq!(
            without_prompt("iya: layoffs on Friday\nboom", prompt),
            "boom"
        );
    }

    #[test]
    fn toml_strings_escape_control_characters() {
        assert_eq!(toml_string("a\nb\u{7}"), "\"a\\u000Ab\\u0007\"");
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
        assert_eq!(CodexHarness::new().command().get_program(), "codex");
        let configured = CodexHarness::with_binary("/opt/codex/bin/codex").command();
        assert_eq!(configured.get_program(), "/opt/codex/bin/codex");
        // Its own folder goes first on the child's `PATH`.
        let path = configured
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .and_then(|(_, value)| value)
            .unwrap();
        assert!(
            std::env::split_paths(path).next().unwrap() == Path::new("/opt/codex/bin"),
            "{path:?}"
        );
        assert_eq!(CodexHarness::new().id(), "codex");
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
