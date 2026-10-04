# Manual checks: tur54

TUR-54: every agent CLI run starts as a `ProcessTree` (process-wrap): a
process group on unix, a Job Object with `CREATE_NO_WINDOW` on Windows. Cancel
and the time limit kill the whole tree. The `/bin/sh` fake harness is replaced
by `fake-cli`, a Rust program in `crates/test-support`, so the agent unit tests
(`process.rs`, `mcp/tests.rs`, `fake.rs`, `tests/fake_harness.rs`) run on every
OS. Tested on this Mac: `cargo test --workspace`, clippy, fmt, and
`cargo check --target x86_64-pc-windows-msvc -p agent -p test-support --tests`.
The Windows and Linux test runs are the `rust (windows)` / `rust (linux)` CI
jobs on the PR.

## Run by hand

1. On Windows, with Claude Code installed through npm (so `claude` is a
   `claude.cmd` shim) and a tracker MCP server configured, start a notes run
   from the app and press Cancel while it runs; then set `agent.timeout_sec`
   to 5 and run again.
   Expect: both stop within about a second, and Task Manager (or
   `tasklist | findstr /i "node cmd"`) shows no `cmd.exe`, `node.exe` or MCP
   server left from the run. No console window flashes up at any point.
   Why skipped: needs a real Windows machine, a signed-in CLI and the running
   app. CI only runs `fake-cli` grandchildren, not a `.cmd` shim.
2. Same on Linux (and NixOS, where `/bin/kill` is missing: the app no longer
   runs it), `ps -ef | grep -E "claude|node"` after Cancel.
   Expect: nothing left from the run.
   Why skipped: needs a real Linux machine and a signed-in CLI.

## Known limits

- process-wrap 10's std `JobObject` does not set
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so if the app itself crashes on
  Windows the tree outlives it. That matches unix, where a process group also
  outlives a crashed parent. Inside the app, a `ProcessTree` dropped before its
  child exited (early return, panic) kills the tree itself. The hand-rolled
  minutes `bounded_child.rs` variant was not copied for this alone.
- `crates/agent/src/detect.rs` keeps its `#[cfg(unix)]` test module and its one
  `R10_DEBT` entry (manager's answer A1: TUR-53 edits that file now). Follow-up:
  move those tests onto `test_support::FakeCli`.
- Still unix-only, not R10 debt (integration tests may gate themselves):
  `crates/agent/tests/claude.rs`, `crates/agent/tests/codex_fake.rs`, and the
  `src-tauri` tests behind `skip_without_fake_cli!` / `FAKE_CLI_RUNS`
  (`src-tauri/src/platform/mod.rs`, not this ticket's file). Follow-up: port
  their `/bin/sh` scripts to `test_support::FakeCli`; `FakeHarness` itself now
  runs on every OS.
