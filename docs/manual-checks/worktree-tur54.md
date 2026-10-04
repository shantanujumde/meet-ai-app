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
2. On Windows, start a notes run, then end `meet-ai.exe` in Task Manager
   (End task) while it runs.
   Expect: the run's `cmd.exe`, `node.exe` and MCP servers are gone within a
   second or two.
   Why skipped: needs a real Windows machine and a signed-in CLI.
3. Same on Linux (and NixOS, where `/bin/kill` is missing: the app no longer
   runs it), `ps -ef | grep -E "claude|node"` after Cancel.
   Expect: nothing left from the run.
   Why skipped: needs a real Linux machine and a signed-in CLI.

## Known limits

- process-wrap 10's std `JobObject` does not set
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so the child is also put in a
  kill-on-close job of our own (`platform/windows_job.rs`). CI's Windows job
  runs `closing_the_job_kills_the_tree_without_a_kill_call`, which closes that
  job without any kill call. On unix a crashed app still leaves its process
  group running (no kill-on-close there).
- `detect.rs`'s tests run `fake-cli` on every OS now and the R10 debt list is
  empty. The four tests about macOS app bundles and the Claude desktop app's
  `Library/Application Support` folder moved to `platform/macos.rs` (Mac-only
  paths; on Windows a copy there would also need an `.exe` ending).
- Still unix-only, not R10 debt (integration tests may gate themselves):
  `crates/agent/tests/claude.rs`, `crates/agent/tests/codex_fake.rs`, and the
  `src-tauri` tests behind `skip_without_fake_cli!` / `FAKE_CLI_RUNS`
  (`src-tauri/src/platform/mod.rs`, not this ticket's file). Follow-up: port
  their `/bin/sh` scripts to `test_support::FakeCli`; `FakeHarness` itself now
  runs on every OS.
