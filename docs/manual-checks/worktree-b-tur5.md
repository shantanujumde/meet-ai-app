# Manual checks: b-tur5

TUR-5: the Codex runner, `crates/agent/src/codex.rs` (`CodexHarness`). It
builds the `codex exec` command lines for the notes run and the sync run
(SPEC A11), passes the schema and reads the reply through temp files, and
lists Codex's models. The tests run against a fake `codex` script
(`crates/agent/tests/codex_fake.rs`); one real-CLI test is `#[ignore]`d
(`crates/agent/tests/codex_real.rs`).

## Run by hand

1. **Real notes run, through the crate.**
   `MEET_AI_CODEX=/Applications/ChatGPT.app/Contents/Resources/codex cargo test -p agent --test codex_real -- --ignored --nocapture`
   Expect: passes in about 15 s, prints the elapsed time; the reply has at
   least one task with an `HH:MM:SS` `transcript_ref`; afterwards
   `ls $TMPDIR | grep meet-ai-` shows no folder left behind.
   Why skipped: needs a signed-in Codex, and agent runs may not run
   `cargo test --ignored`. The same command line was run by hand instead
   (probe 1 below) and passed.
2. **Notes run still works signed in with `--ignore-user-config`**, on a Mac
   whose `~/.codex/config.toml` has MCP servers or plugins (this one has
   `node_repl`, `computer-use` and several plugins). Run check 1 and also
   watch Activity Monitor or `ps -ax | grep -i -e node_repl -e computer-use`
   during the run.
   Expect: the run passes and none of those servers start.
   Why skipped: signed-in CLI. Probe 1 passed with this flag; whether the
   servers stayed down was not watched.
3. **Model picker list.** `CodexHarness::new().models()` (or, once the setup
   screen exists, the Codex model dropdown).
   Expect: `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5`, `gpt-5.2`
   on Codex 0.152.1, in that order, plus the "Codex's default" choice (no
   `--model`). Hidden models (`gpt-5.4`, `codex-auto-review`, ...) are left out.
   Why skipped: needs the real binary on `PATH`. `codex debug models` was run
   by hand once (local, no model call) to get that list and the JSON shape the
   unit test copies.
4. **Codex sync against a real tracker.** Not testable until Phase 4 wires
   Sync. When it is: sync one task to Linear with Codex picked.
   Expect today: no issue is created and the run returns no key (see the
   approval finding below). Expect after a pre-approval is added: one issue,
   its key and URL in the task file.

## MCP approval spike (SPEC A11 note, "Measured 2026-10-02")

Three real-CLI probes, the maximum allowed, all on the 3-line sample (or, for
sync, the one task from it), each under a 120 s alarm, from an empty folder in
`$TMPDIR`, Codex 0.152.1 at `/Applications/ChatGPT.app/Contents/Resources/codex`.
No skip-permissions flag was used.

| # | Command (prompt on stdin) | Result |
|---|---|---|
| 1 | `codex exec --ephemeral --skip-git-repo-check --ignore-user-config --ignore-rules -s read-only --output-schema notes.json -o reply.json -` | exit 0 in 14 s, valid notes JSON, 1 task, working folder empty, model `gpt-5.6-terra` |
| 2 | `codex exec --ephemeral --skip-git-repo-check --ignore-user-config -s read-only -c mcp_servers.probe.command="python3" -c mcp_servers.probe.args=["probe_mcp.py"] --output-schema sync.json -o reply.json -` | exit 0 in 16 s. Banner says `approval: never`. stderr: `MCP tool call requires approval, but approval policy is never`. The server saw `initialize` and `tools/list`, never `tools/call`. Reply `{"external_id":null,"external_url":null}` |
| 3 | as 2, plus `-c mcp_servers.probe.default_tools_approval_mode="approve"` | exit 0 in 13 s. `tools/call` reached the server; reply `PROBE-1` and its URL |

**Nothing to clean up:** the only MCP call that went through was `probe/create_issue`, served by a local fake server (`probe_mcp.py`, launched by Codex over stdio). It returned a canned `PROBE-1` and wrote one log file in the job's temp folder. No tracker, connector or network service was called, and nothing was created or changed outside this Mac.

`probe_mcp.py` was a 40-line stdio MCP server (newline-delimited JSON-RPC)
with one tool, `create_issue(title, description?)`, that logs each request and
returns a fixed key and URL. It touched nothing else. Probes 2 and 3 used
`--ignore-user-config` only so the spike could not reach the user's real MCP
servers; the user's config sets no approval policy, so the default is the
same either way.

Answer to the ticket's question: `exec` does not ask. MCP tool calls are
refused under its fixed `approval: never`, with exit code 0, and the only way
found to allow them without a window is to pre-approve the server's tools in
config. No code in this branch relies on that (per the ticket default); the
SPEC note leaves the choice to whoever wires Sync.

## Known limits

- **A failed tool call is not a failed run.** Probe 2 exited 0. The sync run's
  caller has to treat a reply without a key as "not synced".
- **Codex prints the prompt on stderr.** A failed run's `CliFailed` text (last
  4 KiB of stderr) can contain transcript lines.
- **`enabled_tools` narrows only the servers named in `allowed_tools`.** Other
  MCP servers in the user's config stay available to a sync run. ChatGPT
  connectors ("apps") are not `mcp_servers` and are not narrowed at all.
- **Server names with a `.` (or other non-bare TOML characters)** are written
  quoted, `-c mcp_servers."my.server".enabled_tools=[...]`. Whether Codex's
  `-c` parser honours a quoted dotted-key segment was not checked; if it
  splits on every `.`, such a server is not narrowed. Check with
  `codex exec ... -c 'mcp_servers."my.server".enabled_tools=["x"]'` against a
  server named that way, once Sync is wired.
- **`detect()` returns `None`.** Detection belongs to TUR-6 (`detect::codex`),
  which replaces the stub. Nothing calls `detect()` before wave C.
- **Not signed in** shows as `CliFailed` with Codex's own stderr, not as
  `NotSignedIn`; the exact text Codex prints when logged out was not probed.

## Decisions taken

- **Notes run adds `--ignore-user-config --ignore-rules`** beyond the SPEC
  command. Asked Shann; answer: yes, notes only, sync keeps the user's config.
  Both flags are in `codex exec --help` for 0.152.1.
- **`detect()` is a stub.** Asked Shann; answer: no detection logic here,
  TUR-6 fills it in.
- **Schema and reply go through a separate temp folder** next to the empty
  working folder, so the folder Codex runs in stays empty. Both are deleted
  when the run ends.
- **`models()`** reads `codex debug models` and keeps the `visibility: list`
  entries by priority. On any failure it returns an empty list, and the
  picker offers only "Codex's default".
