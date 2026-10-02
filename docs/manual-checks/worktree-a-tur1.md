# Manual checks: a-tur1

TUR-1: new crate `crates/agent` (SPEC A11) with the `Harness` trait, `Job`,
`AgentError`, the child-process runner (fresh temp folder, prompt on stdin,
time limit, cancel), the schema check and a fake harness behind the
`test-support` feature. Everything here is tested headless with `sh` scripts;
no real agent CLI is involved, because the Claude and Codex runners are later
tickets.

## Run by hand

1. Nothing in the app calls this crate yet, so there is no app flow to try.
   Once the Claude Code runner lands, run the Settings "Test" button (3-line
   sample) with a signed-in `claude`.
   Expect: notes come back, and `ls $TMPDIR | grep meet-ai-agent-` shows no
   folder left behind.
   Why skipped: needs a signed-in CLI and the running app; this ticket has no
   real runner to drive.
2. With the runner in place, start a notes run and press Cancel while it is
   running; then set `agent.timeout_sec` to 5 and run again.
   Expect: both stop within about a second, `ps -ax | grep claude` shows no
   leftover `claude` or MCP server process, and the meeting view shows
   "the run was cancelled" / "did not finish within 5 seconds".
   Why skipped: the group kill is tested with `sh` and `sleep` grandchildren
   only; a real `claude` (node, plus MCP servers it starts) needs the
   signed-in CLI.

## Known limits of the runner

- After a **normal** exit, the process group is killed only if something
  still holds the output pipes open (after a 2 s grace). A helper the CLI
  started that closed its pipes (an MCP server, say) is left running. Check 2
  above is where this would show up with a real `claude`.
- A process that leaves the group (`setsid`) is not killed.
- If a grandchild keeps stdin open without reading it, the thread that writes
  the prompt stays blocked and is leaked. It holds no lock and does not hold up
  the run.
- **If the app crashes or is force-quit mid-run**, nothing kills the child:
  the `claude` / `codex` process (and what it started) runs on until it
  finishes or hits its own limits, and its `meet-ai-agent-*` folder stays in
  `$TMPDIR` (macOS clears that folder on its own over time). To check by hand:
  start a notes run, `kill -9` the app, then `ps -ax | grep claude` and
  `ls $TMPDIR | grep meet-ai-agent-`.
- **`agent.timeout_sec` is not read from config yet.** Only the default
  (`DEFAULT_TIMEOUT_SECS`, 300) exists here. TUR-3 adds the config key, and
  TUR-10 (the automatic notes run with Cancel) builds each `Job` and has to set
  `Job.timeout` from it. Check 2 above depends on that.
- stdout is capped at 8 MiB (past that the run fails as "not valid JSON"), and
  stderr is held as its last 4 KiB while reading, so a runaway CLI cannot fill
  the app's memory.

## Not probed

- The real `claude` CLI was not run (0 of the 3 allowed probes used). Nothing
  in this ticket builds a `claude` command line, so a probe would not have
  tested any code from this branch.

## Decisions taken

- **Schema check uses the `jsonschema` crate** (0.58.4, no default features),
  the same pin TUR-2 adds for `crates/prompts`. Asked Shann; answer was
  `jsonschema`, because the notes schema has a `pattern` on `transcript_ref`.
  A schema failure never quotes the reply: values are masked by jsonschema,
  unexpected key names are counted ("2 unexpected properties") instead of
  listed, and path segments the schema does not declare show as `*`.
- **`work_root` inside a project is refused** (`CouldNotStart`): a `.git`,
  `CLAUDE.md`, `CLAUDE.local.md` or `AGENTS.md` in it or any folder above it.
  It stays public so tests can point it at their own temp folder.
- **One extra `AgentError` variant, `CouldNotStart`,** beyond the seven the
  ticket lists. It covers a working folder that cannot be made, a CLI that
  exists but cannot be launched (for example, not executable), and a broken
  schema. A missing binary is still `NotInstalled`.
- **`Job` has a `model` field** (`Option<String>`), not in the ticket's list,
  because both runners need it and `crates/agent/src/lib.rs` is a shared file
  where later tickets may only add lines.
- **"Working folder" is `Job::work_root`:** the folder each run's new, empty
  temp folder is made in (system temp by default). The temp folder is deleted
  when the run ends.
- **Kill reaches grandchildren** on macOS/Linux: the child gets its own process
  group, and the runner sends `kill -KILL -<pgid>` (`/bin/kill`, so no
  `unsafe` and no `libc` dependency; the crate has `#![forbid(unsafe_code)]`). On Windows only the direct child is
  killed for now.
- `just check` picks the crate up through the `crates/*` workspace glob. It is
  not added to `just check-windows`; that would change an existing justfile
  line, outside this ticket.
