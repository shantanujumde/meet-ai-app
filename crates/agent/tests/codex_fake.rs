//! The Codex harness against a fake `codex`: a `/bin/sh` script that records
//! how it was started and plays back a canned reply. Driven only through
//! `dyn Harness`, the way the app holds the user's pick. Every run goes through
//! the real child-process path.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent::{AgentError, CodexHarness, Harness, Job};
use serde_json::{Value, json};

/// The fake `codex`. `D` is the test's own folder: what the script records
/// goes there, and what it plays back comes from files there, so no canned
/// text is ever pasted into the script.
const SCRIPT: &str = r#"#!/bin/sh
D='@DIR@'
code=0
if [ -f "$D/code.src" ]; then code=$(cat "$D/code.src"); fi
if [ "$1" = debug ] && [ "$2" = models ]; then
  cat >/dev/null
  if [ -f "$D/models.src" ]; then cat "$D/models.src"; fi
  exit "$code"
fi
case " $* " in
  *" mcp list "*)
    printf '%s\n' "$@" > "$D/mcp_args.txt"
    pwd -P > "$D/mcp_cwd.txt"
    if [ -f "$D/mcp.src" ]; then cat "$D/mcp.src"; else echo '[]'; fi
    if [ -f "$D/mcp_code.src" ]; then exit "$(cat "$D/mcp_code.src")"; fi
    exit 0 ;;
esac
printf '%s\n' "$@" > "$D/args.txt"
pwd -P > "$D/cwd.txt"
ls -A | wc -l > "$D/cwd_count.txt"
cat > "$D/stdin.txt"
schema=
reply=
while [ $# -gt 1 ]; do
  case "$1" in
    --output-schema) schema="$2"; shift ;;
    -o) reply="$2"; shift ;;
  esac
  shift
done
if [ -n "$schema" ]; then cp "$schema" "$D/schema_seen.json"; fi
if [ -f "$D/sleep.src" ]; then sleep "$(cat "$D/sleep.src")" </dev/null >/dev/null 2>&1; fi
echo "codex: thinking about the meeting..."
if [ -f "$D/reply.src" ] && [ -n "$reply" ]; then cp "$D/reply.src" "$reply"; fi
echo "codex: done, tokens used: 1234"
if [ -f "$D/echo_prompt.src" ]; then cat "$D/stdin.txt" >&2; echo >&2; fi
if [ -f "$D/stderr.src" ]; then cat "$D/stderr.src" >&2; fi
exit "$code"
"#;

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
/// files the script records never land where the run's folders are made.
struct Fake {
    dir: tempfile::TempDir,
    script: PathBuf,
    root: tempfile::TempDir,
}

impl Fake {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("codex");
        let path = dir.path().canonicalize().unwrap();
        std::fs::write(&script, SCRIPT.replace("@DIR@", path.to_str().unwrap())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            dir,
            script,
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn harness(&self) -> Box<dyn Harness> {
        Box::new(CodexHarness::with_binary(&self.script))
    }

    /// Writes one of the script's `*.src` inputs.
    fn give(&self, name: &str, text: &str) -> &Self {
        std::fs::write(self.dir.path().join(name), text).unwrap();
        self
    }

    /// Reads one of the files the script recorded.
    fn seen(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap()
    }

    fn args(&self) -> Vec<String> {
        self.seen("args.txt").lines().map(String::from).collect()
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
    fake.give("reply.src", &good_notes().to_string());
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
    assert_eq!(fake.seen("stdin.txt"), PROMPT);
    assert!(!args.iter().any(|a| a.contains("Priya")), "{args:?}");

    // Codex got the job's schema as a file.
    let seen: Value = serde_json::from_str(&fake.seen("schema_seen.json")).unwrap();
    assert_eq!(seen, notes_schema());

    // It ran in a fresh, empty folder inside the work root, and the schema
    // and reply files sat in a second folder next to it, not inside it.
    let cwd = PathBuf::from(fake.seen("cwd.txt").trim());
    assert_eq!(cwd.parent().unwrap(), fake.root());
    assert!(name_of(&cwd).starts_with("meet-ai-agent-"), "{cwd:?}");
    assert_eq!(fake.seen("cwd_count.txt").trim(), "0");
    let io = Path::new(&schema).parent().unwrap();
    assert_eq!(Path::new(&reply_file).parent().unwrap(), io);
    assert_eq!(io.parent().unwrap(), fake.root());
    assert!(name_of(io).starts_with("meet-ai-codex-io-"), "{io:?}");
    assert_ne!(io, cwd);

    // Both folders are gone afterwards, and Codex's MCP list was never read.
    assert_empty(fake.root.path());
    assert!(!fake.dir.path().join("mcp_args.txt").exists());
}

#[test]
fn a_notes_run_passes_the_model_only_when_one_is_set() {
    let fake = Fake::new();
    fake.give("reply.src", &good_notes().to_string());
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
    fake.give("reply.src", &reply.to_string())
        .give("mcp.src", MCP_LIST);
    let job = sync_job(&fake, &["mcp__linear__*"]);

    assert_eq!(fake.harness().run(&job).unwrap(), reply);

    // First Codex's own server list, read with the same features off, in a
    // fresh folder of the work root.
    assert_eq!(
        fake.seen("mcp_args.txt").lines().collect::<Vec<_>>(),
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
    let mcp_cwd = PathBuf::from(fake.seen("mcp_cwd.txt").trim());
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
        fake.seen("stdin.txt"),
        "Create an issue: Draft release notes"
    );
    assert_empty(fake.root.path());
}

#[test]
fn a_sync_run_with_listed_tools_pre_approves_each_one() {
    let fake = Fake::new();
    let reply = json!({ "issue_key": "ENG-42", "url": "https://linear.app/acme/issue/ENG-42" });
    fake.give("reply.src", &reply.to_string())
        .give("mcp.src", MCP_LIST);
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
    fake.give("mcp.src", "Error: bad config")
        .give("mcp_code.src", "1");
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
    assert!(!fake.dir.path().join("args.txt").exists(), "exec ran");
    assert_empty(fake.root.path());

    // A list that is not JSON stops it too.
    fake.give("mcp_code.src", "0");
    let err = fake
        .harness()
        .run(&sync_job(&fake, &["mcp__linear__*"]))
        .unwrap_err();
    assert!(matches!(err, AgentError::InvalidJson { .. }), "{err:?}");
    assert!(!fake.dir.path().join("args.txt").exists(), "exec ran");
}

#[test]
fn a_sync_run_to_a_server_codex_does_not_have_never_starts() {
    let fake = Fake::new();
    fake.give("mcp.src", MCP_LIST);
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
        assert!(
            !fake.dir.path().join("args.txt").exists(),
            "{tracker}: exec ran"
        );
    }
    assert_empty(fake.root.path());
}

#[test]
fn a_codex_that_fails_reports_its_exit_code_and_stderr() {
    let fake = Fake::new();
    fake.give("code.src", "3")
        .give("stderr.src", "login expired\n");
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
    fake.give("code.src", "1").give("echo_prompt.src", "").give(
        "stderr.src",
        "OpenAI Codex v0.152.1\nERROR: stream disconnected\n",
    );
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
    fake.give("reply.src", "Sure! Here are your notes: SECRET");
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
    fake.give("reply.src", &reply.to_string());
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
    fake.give("sleep.src", "30")
        .give("reply.src", &good_notes().to_string());
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
    fake.give(
        "models.src",
        r#"{"models":[{"slug":"gpt-5.6-sol","visibility":"list","priority":1},{"slug":"gpt-5.6-terra","visibility":"list","priority":2},{"slug":"gpt-daybreak-blue-latest","visibility":"hide","priority":3},{"slug":"gpt-5.5","visibility":"list","priority":7}]}"#,
    );
    assert_eq!(
        fake.harness().models(),
        ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.5"]
    );
}

#[test]
fn models_are_empty_when_codex_cannot_list_them() {
    let fake = Fake::new();
    fake.give("models.src", "not json at all");
    assert!(fake.harness().models().is_empty());

    fake.give("models.src", r#"{"models":[]}"#)
        .give("code.src", "1");
    assert!(fake.harness().models().is_empty());

    let missing = CodexHarness::with_binary(fake.dir.path().join("no-such-codex"));
    assert!(missing.models().is_empty());
}
