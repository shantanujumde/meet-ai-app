//! The Codex harness against a fake `codex`: `fake-cli` (test-support), which
//! records how it was started and plays back a canned reply, on every OS. Driven only through
//! `dyn Harness`, the way the app holds the user's pick. Every run goes through
//! the real child-process path.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent::{AgentError, CodexHarness, Harness, Job};
use serde_json::{Value, json};

/// How the fake `codex` behaves, as `fake-cli` profiles: `debug models` and
/// `mcp list` get their own settings (`models.*`, `mcp.*`); anything else is
/// `codex exec`. Logs land in the test's folder (`mcp/` for the server list).
fn configure(cli: &test_support::FakeCli, dir: &Path) {
    let mcp = dir.join("mcp");
    std::fs::create_dir(&mcp).unwrap();
    cli.set("profiles", "debug models\tmodels\nmcp list\tmcp\n")
        .set("log_dir", dir.display().to_string())
        .set("save_file_after", "--output-schema")
        .set("reply_file_after", "-o")
        .set(
            "stdout",
            "codex: thinking about the meeting...\ncodex: done, tokens used: 1234\n",
        )
        .set("models.stdout", "")
        .set("models.log_dir", dir.join("models").display().to_string())
        .set("mcp.log_dir", mcp.display().to_string())
        .set("mcp.stdout", "[]")
        .set("mcp.code", "0");
    std::fs::create_dir(dir.join("models")).unwrap();
}

const PROMPT: &str = "Transcript: [00:12:34] Priya: I'll draft the release notes.";

fn notes_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "decisions", "open_questions", "tasks"],
        "properties": {
            "summary": { "type": "string" },
            "decisions": { "type": "array", "items": { "type": "string" } },
            "open_questions": { "type": "array", "items": { "type": "string" } },
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "details", "owner", "due", "transcript_ref"],
                    "properties": {
                        "title": { "type": "string" },
                        "details": { "type": "string" },
                        "owner": { "type": ["string", "null"] },
                        "due": { "type": ["string", "null"] },
                        "transcript_ref": { "type": "string", "pattern": "^\\d{2}:\\d{2}:\\d{2}$" }
                    }
                }
            }
        }
    })
}

fn good_notes() -> Value {
    json!({
        "summary": "Planned the Q3 launch.",
        "decisions": ["Ship on the 14th"],
        "open_questions": ["Who writes the blog post?"],
        "tasks": [{
            "title": "Draft release notes",
            "details": "It's \"done\" now & $HOME stays put.",
            "owner": "Priya",
            "due": null,
            "transcript_ref": "00:12:34"
        }]
    })
}

/// A fake `codex` in a folder of its own, plus a separate work root, so the
/// files the fake records never land where the run's folders are made.
struct Fake {
    dir: tempfile::TempDir,
    cli: test_support::FakeCli,
    root: tempfile::TempDir,
}

impl Fake {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cli = test_support::FakeCli::install(&dir.path().join("bin"), "codex");
        configure(&cli, &dir.path().canonicalize().unwrap());
        Self {
            dir,
            cli,
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn harness(&self) -> Box<dyn Harness> {
        Box::new(CodexHarness::with_binary(self.cli.path()))
    }

    /// The exit code of `codex exec` and `codex debug models`.
    fn exit_code(&self, code: &str) -> &Self {
        self.cli.set("code", code).set("models.code", code);
        self
    }

    /// What `codex exec` prints on stderr.
    fn stderr(&self, text: &str) -> &Self {
        self.cli.set("stderr", text);
        self
    }

    /// How long `codex exec` sleeps before it replies, in seconds.
    fn sleep(&self, secs: &str) -> &Self {
        self.cli.set("sleep", secs);
        self
    }

    /// What `codex exec` writes to its `-o` reply file.
    fn reply(&self, text: &str) -> &Self {
        self.cli.set("reply_file", text);
        self
    }

    /// Makes `codex exec` print the prompt it read on stderr, as Codex does.
    fn echo_prompt(&self) -> &Self {
        self.cli.set("echo_stdin_stderr", "1");
        self
    }

    /// What `codex mcp list --json` prints.
    fn mcp_list(&self, text: &str) -> &Self {
        self.cli.set("mcp.stdout", text);
        self
    }

    /// The exit code of `codex mcp list`.
    fn mcp_exit_code(&self, code: &str) -> &Self {
        self.cli.set("mcp.code", code);
        self
    }

    /// What `codex debug models` prints.
    fn models_list(&self, text: &str) -> &Self {
        self.cli.set("models.stdout", text);
        self
    }

    /// Reads one of the files the fake recorded (`argv.log`, `stdin.log`,
    /// `cwd.log`, `saved_file.log`; `mcp/...` for the server list).
    fn seen(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap()
    }

    fn args(&self) -> Vec<String> {
        self.seen("argv.log").lines().map(String::from).collect()
    }

    /// Whether `codex exec` ran.
    fn exec_ran(&self) -> bool {
        self.dir.path().join("argv.log").exists()
    }

    /// The `codex exec` working folder and how many entries it held.
    fn exec_cwd(&self) -> (PathBuf, String) {
        let report = self.seen("cwd.log");
        let mut lines = report.lines();
        let cwd = PathBuf::from(lines.next().unwrap());
        (cwd, lines.next().unwrap().trim().to_owned())
    }

    fn notes_job(&self) -> Job {
        let mut job = Job::notes(PROMPT, notes_schema());
        job.work_root = self.root.path().to_path_buf();
        job
    }

    fn root(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }
}

fn assert_empty(root: &Path) {
    let left: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(left.is_empty(), "the run left files behind: {left:?}");
}

/// The value after `flag` in `args`.
fn value_after<'a>(args: &'a [String], flag: &str) -> &'a str {
    let at = args.iter().position(|a| a == flag).unwrap();
    &args[at + 1]
}

/// A folder's name, as text.
fn name_of(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn a_notes_run_starts_codex_with_no_user_config_and_returns_the_reply_file() {
    let fake = Fake::new();
    fake.reply(&good_notes().to_string());
    let codex = fake.harness();
    assert_eq!(codex.id(), "codex");

    let reply = codex.run(&fake.notes_job()).unwrap();
    assert_eq!(reply, good_notes());

    let args = fake.args();
    let schema = value_after(&args, "--output-schema").to_owned();
    let reply_file = value_after(&args, "-o").to_owned();
    assert_eq!(
        args,
        [
            "exec",
            "--ephemeral",
            "--skip-git-repo-check",
            "--ignore-user-config",
            "--ignore-rules",
            "-s",
            "read-only",
            "--output-schema",
            &schema,
            "-o",
            &reply_file,
            "-",
        ]
    );

    // The prompt went in on stdin, and nowhere else.
    assert_eq!(fake.seen("stdin.log"), PROMPT);
    assert!(!args.iter().any(|a| a.contains("Priya")), "{args:?}");

    // Codex got the job's schema as a file.
    let seen: Value = serde_json::from_str(&fake.seen("saved_file.log")).unwrap();
    assert_eq!(seen, notes_schema());

    // It ran in a fresh, empty folder inside the work root, and the schema
    // and reply files sat in a second folder next to it, not inside it.
    let (cwd, count) = fake.exec_cwd();
    assert_eq!(cwd.parent().unwrap(), fake.root());
    assert!(name_of(&cwd).starts_with("meet-ai-agent-"), "{cwd:?}");
    assert_eq!(count, "0");
    let io = Path::new(&schema).parent().unwrap();
    assert_eq!(Path::new(&reply_file).parent().unwrap(), io);
    assert_eq!(io.parent().unwrap(), fake.root());
    assert!(name_of(io).starts_with("meet-ai-codex-io-"), "{io:?}");
    assert_ne!(io, cwd);

    // Both folders are gone afterwards, and Codex's MCP list was never read.
    assert_empty(fake.root.path());
    assert!(!fake.dir.path().join("mcp/argv.log").exists());
}

#[test]
fn a_notes_run_passes_the_model_only_when_one_is_set() {
    let fake = Fake::new();
    fake.reply(&good_notes().to_string());
    let codex = fake.harness();

    let mut job = fake.notes_job();
    job.model = Some("gpt-5.6-terra".into());
    codex.run(&job).unwrap();
    let args = fake.args();
    assert_eq!(value_after(&args, "--model"), "gpt-5.6-terra");

    job.model = None;
    codex.run(&job).unwrap();
    assert!(
        !fake.args().contains(&"--model".to_owned()),
        "{:?}",
        fake.args()
    );
    assert_empty(fake.root.path());
}

/// What `codex mcp list --json` prints for the sync tests: the tracker, one
/// other server that is on, and one already off.
const MCP_LIST: &str = r#"[
  {"name":"linear","enabled":true,"auth_status":"unsupported"},
  {"name":"node_repl","enabled":true},
  {"name":"computer-use","enabled":false}
]"#;

fn sync_job(fake: &Fake, tools: &[&str]) -> Job {
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["issue_key", "url"],
        "properties": {
            "issue_key": { "type": "string" },
            "url": { "type": "string" }
        }
    });
    let tools = tools.iter().map(|t| (*t).to_owned()).collect();
    let mut job = Job::sync("Create an issue: Draft release notes", schema, tools);
    job.work_root = fake.root.path().to_path_buf();
    job
}

#[test]
fn a_sync_run_pre_approves_only_the_tracker_and_turns_every_other_server_off() {
    let fake = Fake::new();
    let reply = json!({ "issue_key": "ENG-42", "url": "https://linear.app/acme/issue/ENG-42" });
    fake.reply(&reply.to_string()).mcp_list(MCP_LIST);
    let job = sync_job(&fake, &["mcp__linear__*"]);

    assert_eq!(fake.harness().run(&job).unwrap(), reply);

    // First Codex's own server list, read with the same features off, in a
    // fresh folder of the work root.
    assert_eq!(
        fake.seen("mcp/argv.log").lines().collect::<Vec<_>>(),
        [
            "--disable",
            "apps",
            "--disable",
            "plugins",
            "mcp",
            "list",
            "--json"
        ]
    );
    let mcp_cwd = PathBuf::from(fake.seen("mcp/cwd.log").lines().next().unwrap());
    assert_eq!(mcp_cwd.parent().unwrap(), fake.root());

    // Then the run, with the user's config and one `mcp_servers` setting.
    let args = fake.args();
    assert!(
        !args.contains(&"--ignore-user-config".to_owned()),
        "{args:?}"
    );
    assert!(!args.contains(&"--ignore-rules".to_owned()), "{args:?}");
    assert_eq!(args.iter().filter(|a| *a == "-c").count(), 1, "{args:?}");
    assert_eq!(
        value_after(&args, "-c"),
        r#"mcp_servers={"linear"={default_tools_approval_mode="approve"},"node_repl"={enabled=false}}"#
    );
    let off: Vec<&str> = args
        .iter()
        .zip(args.iter().skip(1))
        .filter(|(flag, _)| *flag == "--disable")
        .map(|(_, feature)| feature.as_str())
        .collect();
    assert_eq!(off, ["apps", "plugins"]);
    assert_eq!(value_after(&args, "-s"), "read-only");
    assert_eq!(args.last().map(String::as_str), Some("-"));
    assert_eq!(
        fake.seen("stdin.log"),
        "Create an issue: Draft release notes"
    );
    assert_empty(fake.root.path());
}

#[test]
fn a_sync_run_with_listed_tools_pre_approves_each_one() {
    let fake = Fake::new();
    let reply = json!({ "issue_key": "ENG-42", "url": "https://linear.app/acme/issue/ENG-42" });
    fake.reply(&reply.to_string()).mcp_list(MCP_LIST);
    let job = sync_job(&fake, &["mcp__linear__create_issue"]);

    assert_eq!(fake.harness().run(&job).unwrap(), reply);
    assert_eq!(
        value_after(&fake.args(), "-c"),
        r#"mcp_servers={"linear"={enabled_tools=["create_issue"],tools={"create_issue"={approval_mode="approve"}}},"node_repl"={enabled=false}}"#
    );
}

#[test]
fn a_sync_run_stops_before_codex_runs_when_the_server_list_cannot_be_read() {
    let fake = Fake::new();
    fake.mcp_list("Error: bad config").mcp_exit_code("1");
    let err = fake
        .harness()
        .run(&sync_job(&fake, &["mcp__linear__*"]))
        .unwrap_err();
    assert!(
        matches!(
            err,
            AgentError::CliFailed {
                status: Some(1),
                ..
            }
        ),
        "{err:?}"
    );
    assert!(!fake.exec_ran(), "exec ran");
    assert_empty(fake.root.path());

    // A list that is not JSON stops it too.
    fake.mcp_exit_code("0");
    let err = fake
        .harness()
        .run(&sync_job(&fake, &["mcp__linear__*"]))
        .unwrap_err();
    assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
    assert!(!fake.exec_ran(), "exec ran");
}

#[test]
fn a_sync_run_to_a_server_codex_does_not_have_never_starts() {
    let fake = Fake::new();
    fake.mcp_list(MCP_LIST);
    for (tracker, words) in [
        ("mcp__jira__*", "Codex has no MCP server named \"jira\""),
        ("mcp__computer-use__*", "is turned off in Codex"),
        ("mcp__my.linear__*", "name contains a dot"),
    ] {
        let err = fake
            .harness()
            .run(&sync_job(&fake, &[tracker]))
            .unwrap_err();
        assert!(
            matches!(&err, AgentError::CouldNotStart { reason } if reason.contains(words)),
            "{tracker}: {err:?}"
        );
        assert!(!fake.exec_ran(), "{tracker}: exec ran");
    }
    assert_empty(fake.root.path());
}

#[test]
fn a_codex_that_fails_reports_its_exit_code_and_stderr() {
    let fake = Fake::new();
    fake.exit_code("3").stderr("login expired\n");
    let err = fake.harness().run(&fake.notes_job()).unwrap_err();
    let AgentError::CliFailed { status, stderr } = &err else {
        panic!("expected CliFailed, got {err:?}")
    };
    assert_eq!(*status, Some(3));
    assert_eq!(stderr, "login expired");
    assert_empty(fake.root.path());
}

#[test]
fn a_failed_run_does_not_pass_on_the_prompt_codex_echoes() {
    let fake = Fake::new();
    fake.exit_code("1")
        .echo_prompt()
        .stderr("OpenAI Codex v0.152.1\nERROR: stream disconnected\n");
    let err = fake.harness().run(&fake.notes_job()).unwrap_err();
    let AgentError::CliFailed { stderr, .. } = &err else {
        panic!("expected CliFailed, got {err:?}")
    };
    assert!(!stderr.contains("Priya"), "{stderr}");
    assert_eq!(stderr, "OpenAI Codex v0.152.1\nERROR: stream disconnected");
}

#[test]
fn a_codex_that_writes_no_reply_file_is_invalid_json() {
    let fake = Fake::new();
    let err = fake.harness().run(&fake.notes_job()).unwrap_err();
    assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
    assert_empty(fake.root.path());
}

#[test]
fn a_reply_file_that_is_not_json_is_invalid_json() {
    let fake = Fake::new();
    fake.reply("Sure! Here are your notes: SECRET");
    let err = fake.harness().run(&fake.notes_job()).unwrap_err();
    assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
    assert!(!err.to_string().contains("SECRET"), "{err}");
    assert_empty(fake.root.path());
}

#[test]
fn a_reply_file_that_breaks_the_schema_is_rejected_without_quoting_it() {
    let fake = Fake::new();
    let mut reply = good_notes();
    reply["tasks"][0]["transcript_ref"] = json!("SECRET-MINUTE");
    fake.reply(&reply.to_string());
    let err = fake.harness().run(&fake.notes_job()).unwrap_err();
    let AgentError::SchemaMismatch { errors } = &err else {
        panic!("expected SchemaMismatch, got {err:?}")
    };
    assert!(
        errors.join("\n").contains("/tasks/0/transcript_ref"),
        "{errors:?}"
    );
    assert!(!err.to_string().contains("SECRET"), "{err}");
    assert_empty(fake.root.path());
}

#[test]
fn a_codex_that_is_not_installed_is_not_found() {
    let fake = Fake::new();
    let codex: Box<dyn Harness> = Box::new(CodexHarness::with_binary(
        fake.dir.path().join("no-such-codex"),
    ));
    let err = codex.run(&fake.notes_job()).unwrap_err();
    assert!(
        matches!(&err, AgentError::NotInstalled { harness } if harness == "Codex"),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "Codex is not installed, or meet-ai cannot find it"
    );
    assert_empty(fake.root.path());
}

#[test]
fn a_run_past_its_time_limit_is_killed() {
    let fake = Fake::new();
    fake.sleep("30").reply(&good_notes().to_string());
    let mut job = fake.notes_job();
    job.timeout = Duration::from_millis(300);
    let start = Instant::now();
    let err = fake.harness().run(&job).unwrap_err();
    let elapsed = start.elapsed();
    assert!(
        matches!(err, AgentError::TimedOut { after } if after == Duration::from_millis(300)),
        "{err:?}"
    );
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    assert_empty(fake.root.path());
}

#[test]
fn models_are_the_listed_ones_from_codex_debug_models() {
    let fake = Fake::new();
    fake.models_list(r#"{"models":[{"slug":"gpt-5.6-sol","visibility":"list","priority":1},{"slug":"gpt-5.6-terra","visibility":"list","priority":2},{"slug":"gpt-daybreak-blue-latest","visibility":"hide","priority":3},{"slug":"gpt-5.5","visibility":"list","priority":7}]}"#,
    );
    assert_eq!(
        fake.harness().models(),
        ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.5"]
    );
}

#[test]
fn models_are_empty_when_codex_cannot_list_them() {
    let fake = Fake::new();
    fake.models_list("not json at all");
    assert!(fake.harness().models().is_empty());

    fake.models_list(r#"{"models":[]}"#).exit_code("1");
    assert!(fake.harness().models().is_empty());

    let missing = CodexHarness::with_binary(fake.dir.path().join("no-such-codex"));
    assert!(missing.models().is_empty());
}
