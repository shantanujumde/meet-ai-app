use super::*;

fn server(name: &str, status: McpStatus) -> McpServer {
    McpServer {
        name: name.to_owned(),
        status,
    }
}

/// `claude mcp list` as Claude Code 2.1.286 printed it on 2026-10-02.
const CLAUDE_SAMPLE: &str = "Checking MCP server health…

claude.ai Claude Docs: https://api.anthropic.com/v1/pages/mcp - ✔ Connected
claude.ai Apollo.io: https://mcp.apollo.io/mcp - ! Needs authentication
claude.ai Linear: https://mcp.linear.app/mcp - ✔ Connected
claude.ai Atlassian: https://mcp.atlassian.com/v1/mcp - ✔ Connected
linear-server: https://mcp.linear.app/mcp (HTTP) - ! Needs authentication
";

/// `codex mcp list --json` as Codex 0.152.1 printed it on 2026-10-02.
const CODEX_SAMPLE: &str = r#"[{"name":"computer-use","enabled":false,"disabled_reason":null,"transport":{"type":"stdio","command":"/Applications/ChatGPT.app/x","args":["mcp"],"env":null,"env_vars":[],"cwd":"."},"startup_timeout_sec":null,"tool_timeout_sec":null,"auth_status":"unsupported"},
 {"name":"node_repl","enabled":true,"disabled_reason":null,"transport":{"type":"stdio","command":"node","args":[],"env":null,"env_vars":[],"cwd":"."},"startup_timeout_sec":null,"tool_timeout_sec":null,"auth_status":"unsupported"}]
"#;

#[test]
fn the_real_claude_list_is_read() {
    assert_eq!(
        parse_claude_list(CLAUDE_SAMPLE),
        [
            server("claude.ai Claude Docs", McpStatus::Connected),
            server("claude.ai Apollo.io", McpStatus::NeedsAuth),
            server("claude.ai Linear", McpStatus::Connected),
            server("claude.ai Atlassian", McpStatus::Connected),
            server("linear-server", McpStatus::NeedsAuth),
        ]
    );
}

#[test]
fn every_claude_status_is_mapped() {
    let out = "a: https://a - ✗ Failed to connect
b: https://b - ⏸ Pending approval
c: https://c - ✓ Connected
d: https://d - Disabled
e: https://e - ! Needs auth
f: https://f - ? Something new
g: https://g - DISCONNECTED
h: https://h - ✔ CONNECTED
";
    assert_eq!(
        parse_claude_list(out),
        [
            server("a", McpStatus::Failed),
            server("b", McpStatus::Pending),
            server("c", McpStatus::Connected),
            server("d", McpStatus::Disabled),
            server("e", McpStatus::NeedsAuth),
            server("f", McpStatus::Unknown),
            server("g", McpStatus::Unknown),
            server("h", McpStatus::Connected),
        ]
    );
}

#[test]
fn a_stdio_command_with_colons_and_dashes_keeps_name_and_status() {
    let out = "my-tool: npx -y some-pkg --flag=a:b - x - y: z - ✔ Connected\n\
               other: node /srv/a - b.js --url http://h:1 - ✗ Failed to connect\n";
    assert_eq!(
        parse_claude_list(out),
        [
            server("my-tool", McpStatus::Connected),
            server("other", McpStatus::Failed),
        ]
    );
}

#[test]
fn an_empty_claude_list_is_empty() {
    assert!(parse_claude_list("").is_empty());
    assert!(parse_claude_list("Checking MCP server health…\n\n").is_empty());
    assert!(
        parse_claude_list("No MCP servers configured. Use `claude mcp add` to add a server.\n")
            .is_empty()
    );
}

#[test]
fn claude_noise_lines_and_repeats_are_skipped() {
    let out = "Checking MCP server health: please wait - ok
Warning: something odd
: https://x - ✔ Connected
linear: https://a - ✔ Connected
linear: https://b - ✗ Failed to connect
";
    assert_eq!(
        parse_claude_list(out),
        [server("linear", McpStatus::Connected)]
    );
}

#[test]
fn the_real_codex_list_is_read() {
    assert_eq!(
        parse_codex_list(CODEX_SAMPLE).unwrap(),
        [
            server("computer-use", McpStatus::Disabled),
            server("node_repl", McpStatus::Configured),
        ]
    );
}

#[test]
fn codex_auth_and_missing_fields_are_mapped() {
    let out = r#"[
        {"name":"linear","enabled":true,"auth_status":"not_logged_in"},
        {"name":"off","enabled":false,"auth_status":"not_logged_in"},
        {"name":"bare"},
        {"name":"oauth","auth_status":"o_auth","extra":{"x":1}},
        {"name":"linear","enabled":false}
    ]"#;
    assert_eq!(
        parse_codex_list(out).unwrap(),
        [
            server("linear", McpStatus::NeedsAuth),
            server("off", McpStatus::Disabled),
            server("bare", McpStatus::Configured),
            server("oauth", McpStatus::Configured),
        ]
    );
    assert!(parse_codex_list("[]\n").unwrap().is_empty());
}

#[test]
fn codex_garbage_is_invalid_json_without_the_text() {
    for out in [
        "",
        "Error: secret-token-123",
        r#"{"servers":[]}"#,
        r#"[{"enabled":true}]"#,
        r#"[{"name":"a","enabled":"secret-token-123"}]"#,
    ] {
        match parse_codex_list(out) {
            Err(AgentError::InvalidJson { reason }) => {
                assert!(!reason.contains("secret"), "{reason}");
                assert!(reason.contains("Codex"), "{reason}");
            }
            other => panic!("{out:?}: expected InvalidJson, got {other:?}"),
        }
    }
}

#[test]
fn statuses_serialize_in_snake_case() {
    let json = serde_json::to_string(&server("x", McpStatus::NeedsAuth)).unwrap();
    assert_eq!(json, r#"{"name":"x","status":"needs_auth"}"#);
}

#[test]
fn claude_tracker_tools_use_the_normalized_name() {
    let id = crate::claude::ID;
    assert_eq!(
        tracker_tools(id, "claude.ai Linear"),
        ["mcp__claude_ai_Linear__*"]
    );
    assert_eq!(
        tracker_tools(id, "linear-server"),
        ["mcp__linear-server__*"]
    );
    assert_eq!(tracker_tools(id, "a/b:c_d"), ["mcp__a_b_c_d__*"]);
}

#[test]
fn codex_tracker_tools_use_the_name_as_is() {
    assert_eq!(tracker_tools("codex", "linear"), ["mcp__linear__*"]);
    assert_eq!(
        tracker_tools("codex", "linear-server"),
        ["mcp__linear-server__*"]
    );
}

#[test]
fn a_blank_server_has_no_tools() {
    for id in [crate::claude::ID, "codex"] {
        assert!(tracker_tools(id, "").is_empty());
        assert!(tracker_tools(id, "  ").is_empty());
    }
}

#[cfg(unix)]
mod fake_cli {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// A fake CLI in a temp folder. It writes its arguments to `args.log`,
    /// its stdin to `stdin.log` and its working folder and how many entries
    /// that folder holds to `cwd.log`, then prints `stdout` when its
    /// arguments are `expected`, and exits 7 otherwise.
    struct FakeCli {
        dir: tempfile::TempDir,
        path: PathBuf,
    }

    impl FakeCli {
        fn new(expected: &str, stdout: &str, exit: i32) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("cli");
            let log = |name: &str| dir.path().join(name).display().to_string();
            let script = format!(
                "#!/bin/sh
printf '%s\\n' \"$*\" > '{args}'
pwd -P > '{cwd}'
ls -A | wc -l >> '{cwd}'
cat > '{stdin}'
if [ \"$*\" != '{expected}' ]; then exit 7; fi
cat <<'MEET_AI_EOF'
{stdout}MEET_AI_EOF
echo 'boom' >&2
exit {exit}
",
                args = log("args.log"),
                cwd = log("cwd.log"),
                stdin = log("stdin.log"),
            );
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            Self { dir, path }
        }

        fn log(&self, name: &str) -> String {
            fs::read_to_string(self.dir.path().join(name)).unwrap()
        }

        /// Checks the CLI got exactly `args`, empty stdin, and an empty
        /// working folder that is not the test's own.
        fn assert_called_with(&self, args: &str) {
            assert_eq!(self.log("args.log"), format!("{args}\n"));
            assert_eq!(self.log("stdin.log"), "");
            let cwd = self.log("cwd.log");
            let mut lines = cwd.lines();
            let folder = PathBuf::from(lines.next().unwrap());
            assert!(
                folder
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("meet-ai-agent-"),
                "{cwd}"
            );
            assert_eq!(lines.next().unwrap().trim(), "0", "{cwd}");
            assert!(!folder.exists(), "the working folder was not deleted");
        }
    }

    const LIMIT: Duration = Duration::from_secs(10);

    #[test]
    fn claude_servers_runs_mcp_list() {
        let cli = FakeCli::new("mcp list", CLAUDE_SAMPLE, 0);
        let servers = claude_servers(&cli.path, LIMIT).unwrap();
        cli.assert_called_with("mcp list");
        assert_eq!(servers, parse_claude_list(CLAUDE_SAMPLE));
        assert_eq!(servers.len(), 5);
    }

    #[test]
    fn codex_servers_runs_mcp_list_json() {
        let cli = FakeCli::new("mcp list --json", CODEX_SAMPLE, 0);
        let servers = codex_servers(&cli.path, LIMIT).unwrap();
        cli.assert_called_with("mcp list --json");
        assert_eq!(
            servers,
            [
                server("computer-use", McpStatus::Disabled),
                server("node_repl", McpStatus::Configured),
            ]
        );
    }

    #[test]
    fn a_failing_cli_is_a_cli_failure() {
        let claude = FakeCli::new("mcp list", CLAUDE_SAMPLE, 1);
        let codex = FakeCli::new("mcp list --json", CODEX_SAMPLE, 1);
        for err in [
            claude_servers(&claude.path, LIMIT).unwrap_err(),
            codex_servers(&codex.path, LIMIT).unwrap_err(),
        ] {
            match err {
                AgentError::CliFailed { status, stderr } => {
                    assert_eq!(status, Some(1));
                    assert_eq!(stderr, "boom");
                }
                other => panic!("expected CliFailed, got {other:?}"),
            }
        }
    }

    #[test]
    fn codex_printing_garbage_is_invalid_json() {
        let cli = FakeCli::new("mcp list --json", "not json\n", 0);
        assert!(matches!(
            codex_servers(&cli.path, LIMIT),
            Err(AgentError::InvalidJson { .. })
        ));
    }

    #[test]
    fn a_missing_cli_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("claude");
        match claude_servers(&missing, LIMIT).unwrap_err() {
            AgentError::NotInstalled { harness } => assert_eq!(harness, "Claude Code"),
            other => panic!("expected NotInstalled, got {other:?}"),
        }
        match codex_servers(&missing, LIMIT).unwrap_err() {
            AgentError::NotInstalled { harness } => assert_eq!(harness, "Codex"),
            other => panic!("expected NotInstalled, got {other:?}"),
        }
    }

    #[test]
    fn a_hanging_list_is_stopped_at_the_time_limit() {
        let cli = FakeCli::new("never", "", 0);
        fs::write(&cli.path, "#!/bin/sh\nsleep 30\n").unwrap();
        let err = claude_servers(&cli.path, Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, AgentError::TimedOut { .. }), "{err:?}");
    }
}
