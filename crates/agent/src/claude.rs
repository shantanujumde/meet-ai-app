//! The Claude Code runner (SPEC A11, "How each CLI is run").
//!
//! Every run is `claude -p --output-format json --json-schema <schema>` with
//! the prompt on stdin. The CLI prints one JSON envelope, and the reply the
//! schema asked for is its `structured_output` field.
//!
//! - A **notes** run gets no tools at all (`--tools ""`), no MCP servers
//!   (`--strict-mcp-config`) and no hooks. All it can do is return text.
//! - A **sync** run keeps the user's MCP servers, but `--allowedTools` lets
//!   it use only the tracker's tools. `--permission-mode dontAsk` turns every
//!   other tool request into a refusal instead of a prompt nobody would see.
//!
//! The prompt always goes in on stdin: with no stdin the CLI waits 3 s and
//! warns "no stdin data received" (measured 2026-10-01).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::process::{self, CliExit};
use crate::{AgentError, Harness, Install, Job, JobKind, OutputCheck, parse_json};

/// The harness id, as written to `agent.harness` and `analyzed_by`.
pub const ID: &str = "claude-code";

/// The CLI's name as the user knows it, for errors and logs.
pub const DISPLAY_NAME: &str = "Claude Code";

/// Claude Code's own settings file, inside its config folder. Its `model`
/// key is the model a run with no `--model` gets (SPEC A14).
const SETTINGS_FILE: &str = "settings.json";

/// Largest settings file read for its `model` key. A real one is a few KiB.
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

/// The binary looked up on `PATH` when no `agent.binary_path` is set.
const BINARY: &str = "claude";

/// Turns off every hook the user has set up, so none runs on a transcript.
const NO_HOOKS: &str = r#"{"disableAllHooks":true}"#;

/// Words in the CLI's error text that mean the model name is wrong, or the
/// account cannot use it.
const MODEL_HINTS: &[&str] = &[
    "issue with the selected model",
    "may not exist or you may not have access",
    "model not found",
    "invalid model",
    "not_found_error",
];

/// Words in the CLI's error text that mean nobody is signed in.
const SIGN_IN_HINTS: &[&str] = &[
    "not logged in",
    "/login",
    "invalid api key",
    "oauth token has expired",
    "authentication_error",
];

/// Runs the user's Claude Code CLI.
#[derive(Debug, Clone, Default)]
pub struct ClaudeHarness {
    /// `agent.binary_path`. `None` looks `claude` up on the search path.
    binary: Option<PathBuf>,
    /// `PATH` the child gets, and the one `claude` is looked up on. `None`
    /// keeps the app's own. An app opened from Finder has a short `PATH`, so
    /// the app passes the login shell's here: the CLI is a node script and
    /// needs `node` on it too.
    search_path: Option<OsString>,
}

impl ClaudeHarness {
    /// Looks `claude` up on the app's `PATH`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs this binary instead of looking `claude` up (`agent.binary_path`).
    pub fn with_binary(mut self, path: impl Into<PathBuf>) -> Self {
        self.binary = Some(path.into());
        self
    }

    /// Sets the `PATH` the child gets and `claude` is looked up on.
    pub fn with_search_path(mut self, path: impl Into<OsString>) -> Self {
        self.search_path = Some(path.into());
        self
    }

    /// The CLI arguments for `job`. The prompt is never among them: it goes
    /// in on stdin, since arguments show up in `ps`.
    ///
    /// Fails for a sync job with no tools, and for a model or tool name that
    /// is empty or would read as a flag.
    pub fn args(&self, job: &Job) -> Result<Vec<OsString>, AgentError> {
        let mut args: Vec<OsString> = vec![
            "-p".into(),
            "--output-format".into(),
            "json".into(),
            "--json-schema".into(),
            job.schema.to_string().into(),
        ];
        if let Some(model) = &job.model {
            check_value("model name", model)?;
            args.extend(["--model".into(), model.into()]);
        }
        match job.kind {
            JobKind::Notes => {
                args.extend(["--tools".into(), "".into(), "--strict-mcp-config".into()]);
            }
            JobKind::Sync => args.extend(["--allowedTools".into(), allowed_tools(job)?.into()]),
        }
        args.extend([
            "--permission-mode".into(),
            "dontAsk".into(),
            "--settings".into(),
            NO_HOOKS.into(),
        ]);
        Ok(args)
    }

    fn command(&self, args: Vec<OsString>) -> Command {
        let mut command = Command::new(self.binary.as_deref().unwrap_or(BINARY.as_ref()));
        command.args(args);
        if let Some(path) = &self.search_path {
            // With `PATH` set on the command, std looks the binary up on it.
            command.env("PATH", path);
        }
        command
    }
}

impl Harness for ClaudeHarness {
    fn id(&self) -> &'static str {
        ID
    }

    fn detect(&self) -> Option<Install> {
        crate::detect::claude(self.binary.as_deref())
    }

    /// The list in `models.json`: Claude Code cannot list its models
    /// without starting a session. Nothing is run for it.
    fn models(&self) -> Vec<String> {
        crate::models::listed(ID)
            .into_iter()
            .map(|model| model.name)
            .collect()
    }

    fn run(&self, job: &Job) -> Result<serde_json::Value, AgentError> {
        let args = self.args(job)?;
        let check = OutputCheck::new(&job.schema)?;
        let dir = process::fresh_work_dir(job)?;
        let exit = process::run_cli_exit(DISPLAY_NAME, self.command(args), job, dir.path())?;
        drop(dir);
        check.check(reply(exit)?)
    }
}

/// The model Claude Code runs when meet-ai passes no `--model`, as far as
/// its user settings say: the `model` key of `settings.json` in its config
/// folder (`$CLAUDE_CONFIG_DIR`, else `~/.claude`). `None` when there is no
/// such file or key; Claude Code then uses its built-in default, which it
/// does not write anywhere. Only read, never written. `ANTHROPIC_MODEL` and
/// project settings can still override it; a notes run has no project.
pub fn settings_model() -> Option<String> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home_dir()?.join(".claude"),
    };
    settings_model_in(&dir)
}

/// [`settings_model`] for the config folder `dir`. A missing, unreadable,
/// oversized or invalid file is `None`, as is a blank or non-text `model`.
pub fn settings_model_in(dir: &Path) -> Option<String> {
    let path = dir.join(SETTINGS_FILE);
    let size = std::fs::metadata(&path).ok()?.len();
    if size > MAX_SETTINGS_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let settings: serde_json::Value = serde_json::from_str(&text).ok()?;
    let model = settings.get("model")?.as_str()?.trim();
    (!model.is_empty()).then(|| model.to_owned())
}

/// The user's home folder: `HOME`, or `USERPROFILE` on Windows.
fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// `job.allowed_tools` as one comma-separated `--allowedTools` value.
fn allowed_tools(job: &Job) -> Result<String, AgentError> {
    if job.allowed_tools.is_empty() {
        return Err(could_not_start(
            "a sync run needs the tracker's tools, and none were given",
        ));
    }
    for tool in &job.allowed_tools {
        check_value("tool name", tool)?;
        if tool.contains(',') {
            return Err(could_not_start(format!(
                "the tool name {tool:?} has a comma in it"
            )));
        }
    }
    Ok(job.allowed_tools.join(","))
}

/// Refuses an empty value, or one the CLI would read as a flag.
fn check_value(what: &str, value: &str) -> Result<(), AgentError> {
    if value.trim().is_empty() {
        return Err(could_not_start(format!("the {what} is empty")));
    }
    if value.starts_with('-') {
        return Err(could_not_start(format!(
            "the {what} {value:?} starts with '-'"
        )));
    }
    Ok(())
}

fn could_not_start(reason: impl Into<String>) -> AgentError {
    AgentError::CouldNotStart {
        reason: reason.into(),
    }
}

/// The JSON envelope `claude -p --output-format json` prints. Only the fields
/// read here; the CLI adds others (cost, usage, session id).
#[derive(Debug, serde::Deserialize)]
struct Envelope {
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    structured_output: Option<serde_json::Value>,
}

impl Envelope {
    fn is_error(&self) -> bool {
        self.is_error
            || self
                .subtype
                .as_deref()
                .is_some_and(|subtype| subtype.starts_with("error"))
    }

    /// Whether the error text says nobody is signed in.
    fn says_not_signed_in(&self) -> bool {
        let text = self.result.as_deref().unwrap_or_default().to_lowercase();
        SIGN_IN_HINTS.iter().any(|hint| text.contains(hint))
    }

    /// Whether the error text says the model is unknown or not allowed.
    fn says_bad_model(&self) -> bool {
        let text = self.result.as_deref().unwrap_or_default().to_lowercase();
        MODEL_HINTS.iter().any(|hint| text.contains(hint))
    }

    /// The error for an envelope that reports one. Its `result` text is only
    /// read, never quoted: it can carry model text.
    fn error(&self, exit: &CliExit) -> AgentError {
        if self.says_not_signed_in() {
            return AgentError::NotSignedIn {
                harness: DISPLAY_NAME.to_owned(),
            };
        }
        if self.says_bad_model() {
            return AgentError::CliFailed {
                status: exit.code,
                stderr: format!(
                    "{DISPLAY_NAME} cannot use this model: the name is wrong, or this \
                     account has no access to it. Pick another model, or Default"
                ),
            };
        }
        let stderr = if exit.stderr.is_empty() {
            format!(
                "{DISPLAY_NAME} reported an error ({})",
                self.subtype.as_deref().unwrap_or("no details")
            )
        } else {
            exit.stderr.clone()
        };
        AgentError::CliFailed {
            status: exit.code,
            stderr,
        }
    }
}

/// The `structured_output` of a finished run, not yet checked against the
/// schema.
fn reply(exit: CliExit) -> Result<serde_json::Value, AgentError> {
    let envelope = serde_json::from_str::<Envelope>(exit.stdout.trim()).ok();
    if !exit.success() {
        return Err(match envelope {
            Some(envelope) if envelope.is_error() => envelope.error(&exit),
            _ => AgentError::CliFailed {
                status: exit.code,
                stderr: exit.stderr,
            },
        });
    }
    if exit.stdout_overflowed {
        return Err(AgentError::InvalidJson {
            reason: "the reply was too large".to_owned(),
        });
    }
    let value = parse_json(&exit.stdout)?;
    let envelope: Envelope =
        serde_json::from_value(value).map_err(|_| AgentError::InvalidJson {
            reason: format!("the reply was not {DISPLAY_NAME}'s JSON result"),
        })?;
    if envelope.is_error() {
        return Err(envelope.error(&exit));
    }
    envelope
        .structured_output
        .ok_or_else(|| AgentError::InvalidJson {
            reason: "the reply had no structured_output".to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit(code: i32, stdout: &str, stderr: &str) -> CliExit {
        CliExit {
            code: Some(code),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            stdout_overflowed: false,
        }
    }

    #[test]
    fn a_not_logged_in_envelope_on_a_failed_exit_means_not_signed_in() {
        let stdout = r#"{"type":"result","subtype":"success","is_error":true,
            "result":"Not logged in · Please run /login"}"#;
        let err = reply(exit(1, stdout, "")).unwrap_err();
        assert!(matches!(err, AgentError::NotSignedIn { .. }), "{err:?}");
    }

    #[test]
    fn an_error_envelope_never_quotes_its_result_text() {
        let stdout = r#"{"subtype":"error_during_execution","is_error":true,
            "result":"SECRET transcript words"}"#;
        let err = reply(exit(1, stdout, "")).unwrap_err().to_string();
        assert!(!err.contains("SECRET"), "{err}");
        assert!(err.contains("error_during_execution"), "{err}");
        assert!(err.contains("exit code 1"), "{err}");
    }

    #[test]
    fn an_unknown_model_says_so_without_quoting_the_result() {
        let stdout = r#"{"type":"result","subtype":"success","is_error":true,
            "result":"There's an issue with the selected model (claude-nope). It may not exist or you may not have access to it."}"#;
        let err = reply(exit(1, stdout, "")).unwrap_err().to_string();
        assert!(err.contains("cannot use this model"), "{err}");
        assert!(!err.contains("claude-nope"), "{err}");
    }

    #[test]
    fn the_settings_model_is_read_and_anything_odd_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(settings_model_in(dir.path()), None, "no file");

        let file = dir.path().join(SETTINGS_FILE);
        for (text, want) in [
            (r#"{"model":"sonnet"}"#, Some("sonnet")),
            (
                r#"{"model":"  claude-haiku-4-5 "}"#,
                Some("claude-haiku-4-5"),
            ),
            (r#"{"theme":"dark"}"#, None),
            (r#"{"model":""}"#, None),
            (r#"{"model":42}"#, None),
            ("not json {", None),
            ("[]", None),
        ] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(settings_model_in(dir.path()).as_deref(), want, "{text}");
        }
        // Read only: the file is as it was written.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[]");
    }

    #[test]
    fn a_failed_exit_with_stdout_that_is_not_an_envelope_keeps_stderr() {
        let err = reply(exit(2, "SECRET", "bad flag")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the agent CLI failed (exit code 2): bad flag"
        );
    }

    #[test]
    fn an_overflowed_reply_is_rejected() {
        let mut exit = exit(0, "", "");
        exit.stdout_overflowed = true;
        let err = reply(exit).unwrap_err();
        assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
    }

    #[test]
    fn notes_args_ignore_any_tools_on_the_job() {
        let mut job = Job::notes("x", serde_json::json!({}));
        job.allowed_tools = vec!["Bash".into()];
        let args = ClaudeHarness::new().args(&job).unwrap();
        assert!(!args.iter().any(|arg| arg == "Bash"), "{args:?}");
        assert!(!args.iter().any(|arg| arg == "--allowedTools"), "{args:?}");
    }

    #[test]
    fn a_tool_name_with_a_comma_is_refused() {
        let job = Job::sync("x", serde_json::json!({}), vec!["a,Bash".into()]);
        let err = ClaudeHarness::new().args(&job).unwrap_err();
        assert!(matches!(err, AgentError::CouldNotStart { .. }), "{err:?}");
    }
}
