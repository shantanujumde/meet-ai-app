//! The Claude Code runner, driven against a fake `claude` on `PATH`.
//!
//! The fake is `fake-cli` (test-support), which records what it was given (its
//! arguments, stdin and working folder) and prints whatever the test told it
//! to. That checks the exact command line and the reply handling without the
//! real CLI or a network.
//!
//! Two `#[ignore]`d tests at the bottom run the real, signed-in `claude`. They
//! cost money and need a login, so they only run by hand.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent::{AgentError, ClaudeHarness, Harness, Job};
use serde_json::{Value, json};

/// A fake `claude` in its own temp folder, plus a separate work root for the
/// runs. Both are deleted when it is dropped.
struct Fake {
    home: tempfile::TempDir,
    bin: PathBuf,
    cli: test_support::FakeCli,
    work_root: tempfile::TempDir,
}

impl Fake {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let cli = test_support::FakeCli::install(&bin, "claude");
        cli.set("log_dir", home.path().display().to_string());
        Self {
            home,
            bin,
            cli,
            work_root: tempfile::tempdir().unwrap(),
        }
    }

    /// Absolute path to the fake binary.
    fn binary(&self) -> PathBuf {
        self.cli.path().to_path_buf()
    }

    /// A harness that finds the fake by name, on its `PATH`.
    fn harness(&self) -> ClaudeHarness {
        ClaudeHarness::new().with_search_path(search_path(&self.bin))
    }

    /// What the fake prints on stdout.
    fn stdout(&self, text: &str) -> &Self {
        self.cli.set("stdout", text);
        self
    }

    /// What the fake prints on stderr.
    fn stderr(&self, text: &str) -> &Self {
        self.cli.set("stderr", text);
        self
    }

    /// The fake's exit code.
    fn exit_code(&self, code: i32) -> &Self {
        self.cli.set("code", code.to_string());
        self
    }

    /// A notes job that runs in this fake's work root.
    fn notes_job(&self, prompt: &str) -> Job {
        let mut job = Job::notes(prompt, schema());
        job.work_root = self.work_root.path().to_path_buf();
        job.timeout = Duration::from_secs(10);
        job
    }

    /// A sync job that runs in this fake's work root.
    fn sync_job(&self, tools: &[&str]) -> Job {
        let mut job = Job::sync(
            "file this task",
            schema(),
            tools.iter().map(|t| (*t).to_owned()).collect(),
        );
        job.work_root = self.work_root.path().to_path_buf();
        job.timeout = Duration::from_secs(10);
        job
    }

    /// The arguments the fake was started with, or `None` if it never ran.
    fn args(&self) -> Option<Vec<String>> {
        let text = fs::read_to_string(self.log("argv.log")).ok()?;
        let text = text.strip_suffix('\n').unwrap_or(&text);
        Some(text.split('\n').map(str::to_owned).collect())
    }

    /// Everything the fake read from stdin.
    fn stdin(&self) -> String {
        fs::read_to_string(self.log("stdin.log")).unwrap()
    }

    /// The folder the fake ran in.
    fn cwd(&self) -> PathBuf {
        let report = fs::read_to_string(self.log("cwd.log")).unwrap();
        PathBuf::from(report.lines().next().unwrap())
    }

    /// How many entries the folder the fake ran in held.
    fn entries_in_cwd(&self) -> usize {
        let report = fs::read_to_string(self.log("cwd.log")).unwrap();
        report.lines().nth(1).unwrap().trim().parse().unwrap()
    }

    /// Where the fake writes one of its logs.
    fn log(&self, name: &str) -> PathBuf {
        self.home.path().join(name)
    }
}

/// A `PATH` of just `bin`: `fake-cli` needs nothing else.
fn search_path(bin: &Path) -> OsString {
    std::env::join_paths([bin]).unwrap()
}

/// A small, strict reply schema.
fn schema() -> Value {
    json!({
        "type": "object",
        "properties": { "summary": { "type": "string" } },
        "required": ["summary"],
        "additionalProperties": false
    })
}

/// The schema as the CLI gets it: one line of compact JSON.
fn schema_arg() -> String {
    serde_json::to_string(&schema()).unwrap()
}

/// The JSON envelope `claude -p --output-format json` prints on success.
fn envelope(structured_output: &Value) -> String {
    json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": "",
        "session_id": "x",
        "total_cost_usd": 0.01,
        "structured_output": structured_output
    })
    .to_string()
}

/// The envelope the CLI prints when the run itself failed.
fn error_envelope(subtype: &str, result: &str) -> String {
    json!({
        "type": "result",
        "subtype": subtype,
        "is_error": true,
        "result": result,
        "session_id": "x",
        "total_cost_usd": 0.0
    })
    .to_string()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn expect_could_not_start(result: Result<Value, AgentError>) -> String {
    match result {
        Err(AgentError::CouldNotStart { reason }) => reason,
        other => panic!("expected CouldNotStart, got {other:?}"),
    }
}

#[test]
fn a_notes_run_sends_the_exact_flags_and_the_prompt_only_on_stdin() {
    let fake = Fake::new();
    let reply = json!({ "summary": "We ship Friday." });
    fake.stdout(&envelope(&reply));
    let prompt = "Write notes for this meeting.\n\
                  [00:00:01] Ana: \"Ship it\" -- won't we? $HOME `id` 'quoted'\n\
                  [00:00:09] Ben: ok\n";
    let mut job = fake.notes_job(prompt);
    job.model = Some("haiku".into());

    let out = fake.harness().run(&job).unwrap();
    assert_eq!(out, reply);

    let expected = strings(&[
        "-p",
        "--output-format",
        "json",
        "--json-schema",
        &schema_arg(),
        "--model",
        "haiku",
        "--tools",
        "",
        "--strict-mcp-config",
        "--permission-mode",
        "dontAsk",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ]);
    let args = fake.args().expect("the fake never ran");
    assert_eq!(args, expected);
    // `args()` on the harness is what `run` used.
    let built: Vec<OsString> = fake.harness().args(&job).unwrap();
    assert_eq!(
        built,
        expected.iter().map(OsString::from).collect::<Vec<_>>()
    );

    // Byte for byte, with no shell expansion of `$HOME` or the backticks.
    assert_eq!(fake.stdin(), prompt);
    assert!(
        args.iter()
            .all(|a| !a.contains("Ana:") && !a.contains("Write notes")),
        "the prompt leaked into the arguments: {args:?}"
    );

    // It ran in a new, empty `meet-ai-agent-*` folder under the work root,
    // which is gone now.
    let cwd = fake.cwd();
    assert_eq!(
        cwd.parent().unwrap(),
        fake.work_root.path().canonicalize().unwrap()
    );
    let name = cwd.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("meet-ai-agent-"), "{name}");
    assert_eq!(fake.entries_in_cwd(), 0, "the working folder was not empty");
    assert!(!cwd.exists(), "the working folder was not deleted");
    assert_eq!(fs::read_dir(fake.work_root.path()).unwrap().count(), 0);
}

#[test]
fn a_notes_run_without_a_model_lets_the_cli_pick() {
    let fake = Fake::new();
    fake.stdout(&envelope(&json!({ "summary": "s" })));
    fake.harness().run(&fake.notes_job("p")).unwrap();

    let args = fake.args().unwrap();
    assert!(!args.iter().any(|a| a == "--model"), "{args:?}");
    assert!(args.iter().any(|a| a == "--tools"), "{args:?}");
}

#[test]
fn a_sync_run_allows_only_its_tools() {
    let fake = Fake::new();
    let reply = json!({ "summary": "LIN-1" });
    fake.stdout(&envelope(&reply));
    let mut job = fake.sync_job(&["mcp__linear__create_issue", "mcp__linear__get_issue"]);
    job.model = Some("sonnet".into());

    assert_eq!(fake.harness().run(&job).unwrap(), reply);
    let args = fake.args().unwrap();
    assert_eq!(
        args,
        strings(&[
            "-p",
            "--output-format",
            "json",
            "--json-schema",
            &schema_arg(),
            "--model",
            "sonnet",
            "--allowedTools",
            "mcp__linear__create_issue,mcp__linear__get_issue",
            "--permission-mode",
            "dontAsk",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ])
    );
    assert!(!args.iter().any(|a| a == "--tools"), "{args:?}");
    assert!(!args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
    assert_eq!(fake.stdin(), "file this task");
}

#[test]
fn a_sync_run_with_no_tools_never_starts() {
    let fake = Fake::new();
    fake.stdout(&envelope(&json!({ "summary": "s" })));
    let job = fake.sync_job(&[]);

    expect_could_not_start(fake.harness().run(&job));
    assert!(fake.args().is_none(), "the fake was started");
    assert!(fake.harness().args(&job).is_err());
}

#[test]
fn a_model_that_looks_like_a_flag_never_starts() {
    let fake = Fake::new();
    fake.stdout(&envelope(&json!({ "summary": "s" })));
    let mut job = fake.notes_job("p");
    job.model = Some("--dangerously-skip-permissions".into());

    expect_could_not_start(fake.harness().run(&job));
    assert!(fake.args().is_none(), "the fake was started");
}

#[test]
fn blank_models_and_flag_like_or_blank_tools_are_refused() {
    let fake = Fake::new();
    let harness = fake.harness();
    for model in ["", "   "] {
        let mut job = fake.notes_job("p");
        job.model = Some(model.into());
        assert!(harness.args(&job).is_err(), "model {model:?}");
    }
    for tool in ["-x", "--dangerously-skip-permissions", "", " "] {
        let job = fake.sync_job(&["mcp__linear__get_issue", tool]);
        assert!(harness.args(&job).is_err(), "tool {tool:?}");
    }
    assert!(fake.args().is_none(), "the fake was started");
}

#[test]
fn a_non_zero_exit_is_a_cli_failure_with_its_stderr() {
    let fake = Fake::new();
    fake.stderr("boom").exit_code(3);
    let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
    match err {
        AgentError::CliFailed { status, stderr } => {
            assert_eq!(status, Some(3));
            assert!(stderr.contains("boom"), "{stderr}");
        }
        other => panic!("expected CliFailed, got {other:?}"),
    }
}

#[test]
fn stdout_that_is_not_json_is_invalid_json() {
    for stdout in ["not json", ""] {
        let fake = Fake::new();
        fake.stdout(stdout);
        let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
        assert!(
            matches!(err, AgentError::InvalidJson { .. }),
            "{stdout:?}: {err:?}"
        );
    }
}

#[test]
fn an_envelope_without_structured_output_is_invalid_json() {
    let without = json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": "Here are your notes",
        "session_id": "x"
    })
    .to_string();
    let null = envelope(&Value::Null);
    for stdout in [without, null] {
        let fake = Fake::new();
        fake.stdout(&stdout);
        let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
        assert!(
            matches!(err, AgentError::InvalidJson { .. }),
            "{stdout}: {err:?}"
        );
    }
}

#[test]
fn structured_output_that_breaks_the_schema_is_a_mismatch() {
    let fake = Fake::new();
    fake.stdout(&envelope(&json!({ "summary": 42, "extra": true })));
    let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
    assert!(matches!(err, AgentError::SchemaMismatch { .. }), "{err:?}");
}

#[test]
fn an_error_envelope_about_login_is_not_signed_in() {
    let fake = Fake::new();
    fake.stdout(&error_envelope(
        "success",
        "Not logged in · Please run /login",
    ));
    let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
    match err {
        AgentError::NotSignedIn { harness } => assert_eq!(harness, "Claude Code"),
        other => panic!("expected NotSignedIn, got {other:?}"),
    }
}

#[test]
fn a_login_error_envelope_wins_over_a_non_zero_exit() {
    let fake = Fake::new();
    fake.stdout(&error_envelope(
        "success",
        "Not logged in · Please run /login",
    ))
    .exit_code(1);
    let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
    match err {
        AgentError::NotSignedIn { harness } => assert_eq!(harness, "Claude Code"),
        other => panic!("expected NotSignedIn, got {other:?}"),
    }
}

#[test]
fn a_non_zero_exit_with_stdout_that_is_not_an_envelope_keeps_its_stderr() {
    let fake = Fake::new();
    fake.stdout("not json").stderr("boom").exit_code(1);
    let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
    match err {
        AgentError::CliFailed { status, stderr } => {
            assert_eq!(status, Some(1));
            assert_eq!(stderr, "boom");
        }
        other => panic!("expected CliFailed, got {other:?}"),
    }
}

#[test]
fn any_other_error_envelope_is_a_cli_failure_that_names_its_subtype_only() {
    let fake = Fake::new();
    fake.stdout(&error_envelope("error_during_execution", "API Error: 500"));
    let err = fake.harness().run(&fake.notes_job("p")).unwrap_err();
    match &err {
        AgentError::CliFailed { status, stderr } => {
            assert_eq!(*status, Some(0));
            assert!(stderr.contains("error_during_execution"), "{stderr}");
            // `result` can carry model text, which may quote the transcript.
            assert!(!stderr.contains("API Error: 500"), "{stderr}");
        }
        other => panic!("expected CliFailed, got {other:?}"),
    }
    let message = err.to_string();
    assert!(message.contains("error_during_execution"), "{message}");
    assert!(!message.contains("API Error: 500"), "{message}");
}

#[test]
fn a_missing_binary_is_not_installed() {
    let fake = Fake::new();
    let empty = tempfile::tempdir().unwrap();
    let harness = ClaudeHarness::new().with_search_path(search_path(empty.path()));
    let err = harness.run(&fake.notes_job("p")).unwrap_err();
    match err {
        AgentError::NotInstalled { harness } => assert_eq!(harness, "Claude Code"),
        other => panic!("expected NotInstalled, got {other:?}"),
    }
    assert!(fake.args().is_none(), "the fake was started");
}

#[test]
fn a_binary_path_override_works_without_it_on_the_path() {
    let fake = Fake::new();
    let reply = json!({ "summary": "s" });
    fake.stdout(&envelope(&reply));
    let harness = ClaudeHarness::new()
        .with_binary(fake.binary())
        .with_search_path(search_path(tempfile::tempdir().unwrap().path()));
    assert_eq!(harness.run(&fake.notes_job("p")).unwrap(), reply);
    assert!(fake.args().is_some());
}

#[test]
fn id_and_models() {
    let harness = ClaudeHarness::default();
    assert_eq!(harness.id(), "claude-code");
    assert_eq!(harness.id(), agent::claude::ID);
    // From models.json: Sonnet and Haiku first (the setup screen's buttons).
    assert_eq!(&harness.models()[..3], ["sonnet", "haiku", "opus"]);
    assert_eq!(agent::claude::DISPLAY_NAME, "Claude Code");
}

// The real CLI. Run by hand with
// `cargo test -p agent --test claude -- --ignored`.

/// The strict notes schema from SPEC A11.
fn real_notes_schema() -> Value {
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
                        "transcript_ref": { "type": "string" }
                    }
                }
            }
        }
    })
}

#[test]
#[ignore = "runs the real, signed-in claude CLI"]
fn real_cli_notes_run_on_three_line_transcript() {
    let root = tempfile::tempdir().unwrap();
    let prompt = "Write meeting notes for the transcript below. List every task \
                  someone took on and every question left open. Use the \
                  timestamp of the line a task comes from as its transcript_ref.\n\n\
                  [00:00:01] Ana: Let's ship the beta Friday.\n\
                  [00:00:09] Ben: I'll write the release notes by Thursday.\n\
                  [00:00:15] Ana: Do we need legal sign-off?\n";
    let mut job = Job::notes(prompt, real_notes_schema());
    job.model = Some("haiku".into());
    job.work_root = root.path().to_path_buf();
    job.timeout = Duration::from_secs(120);

    let notes = ClaudeHarness::new().run(&job).unwrap();
    let tasks = notes["tasks"].as_array().unwrap();
    let questions = notes["open_questions"].as_array().unwrap();
    assert!(!tasks.is_empty(), "{notes:#}");
    assert!(!questions.is_empty(), "{notes:#}");
}

#[test]
#[ignore = "runs the real, signed-in claude CLI"]
fn real_cli_sync_run_refuses_bash() {
    let root = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let target = target_dir.path().join("bash-ran");
    let prompt = format!(
        "Use the Bash tool to run this exact command: touch {}\n\
         Then reply with ran_bash true if the command ran, false if you could \
         not run it, and a short note saying why.",
        target.display()
    );
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ran_bash", "note"],
        "properties": {
            "ran_bash": { "type": "boolean" },
            "note": { "type": "string" }
        }
    });
    let mut job = Job::sync(
        prompt,
        schema,
        vec!["mcp__claude_ai_Linear__list_issues".into()],
    );
    job.model = Some("haiku".into());
    job.work_root = root.path().to_path_buf();
    job.timeout = Duration::from_secs(120);

    let result = ClaudeHarness::new().run(&job);
    // Check the side effect, not what the model says it did. Any reply, or a
    // failed or off-format one, is fine as long as Bash never ran.
    match &result {
        Ok(_)
        | Err(AgentError::CliFailed { .. })
        | Err(AgentError::SchemaMismatch { .. })
        | Err(AgentError::InvalidJson { .. }) => {}
        Err(other) => panic!("the run did not get far enough to test anything: {other:?}"),
    }
    assert!(
        !target.exists(),
        "Bash ran in a sync job that only allowed a Linear tool: {result:?}"
    );
}
