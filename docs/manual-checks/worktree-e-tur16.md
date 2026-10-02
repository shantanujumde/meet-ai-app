# Manual checks: e-tur16

TUR-16: a Codex sync run can reach only the tracker's MCP tools (SPEC A11,
"Measured 2026-10-02, later"). Before `codex exec`, the run reads
`codex mcp list --json` to learn every MCP server Codex will load. It then
passes one `-c mcp_servers={...}` that pre-approves the tracker's tools and
turns every other server off, plus `--disable apps --disable plugins`. The
code is in `crates/agent/src/codex.rs`, with tests in
`crates/agent/tests/codex_fake.rs`. The fake `codex` script in
`src-tauri/src/sync/tests.rs` gained a few lines so it answers `mcp list`.

## Run by hand

1. **Codex sync of one task to a real tracker.** In the app, pick Codex in
   Settings, with Linear set up in `~/.codex/config.toml`. Open a meeting
   and press Sync on one task. Then press Sync on it again.
   Expect: exactly one Linear issue is created. Its key and URL are written to
   the ticket's `external_id` and `external_url`. The second Sync is refused
   because the task is already synced.
   Why skipped: needs a signed-in Codex and a real tracker, and agent runs
   must never run a real sync.
2. **No other MCP server starts during that run.** Do check 1 on a Mac whose
   `~/.codex/config.toml` has other servers (this one has `node_repl` and
   `computer-use`). During the run, watch
   `ps -ax | grep -i -e node_repl -e computer-use`.
   Optional: add a second local fake server to the config and check that its
   log file stays empty.
   Expect: none of the other servers start; the fake's log stays empty.
   Why skipped: signed-in CLI and a real tracker, as in check 1. The local
   checks below show an `enabled=false` server never starts.
3. **Connectors stay off.** On an account that has ChatGPT connectors set up,
   run a sync and read Codex's stderr (its progress output).
   Then run the same command without `--disable apps`.
   Expect: with the flag, no connector tool and no `codex_apps` server
   shows up. Without it, they do. That shows the flag is what keeps them out.
   Why skipped: needs a signed-in account with connectors. This account's
   connector list was not checked without the flag.
4. **A tracker name with a dot.** Rename the tracker's server in
   `~/.codex/config.toml` to one with a `.` (say `linear.work`), point
   `tickets.tracker_mcp` at it, and press Sync.
   Expect: no run starts. Sync shows "Codex can't use a tracker server whose
   name contains a dot. Rename it in ~/.codex/config.toml."
   Why skipped: needs the app running with a signed-in Codex. The fake-script
   tests cover the refusal.
5. **The server list is quick.** Signed in, with the real config, run
   `time codex --disable apps --disable plugins mcp list --json`.
   Expect: it finishes well under its time limit (60 s, or
   `agent.timeout_sec` if that is lower) and prints every server in the
   config.
   Why skipped: signed-in CLI with the user's real config.

## Probes

All on Codex 0.152.1 at `/Applications/ChatGPT.app/Contents/Resources/codex`.

**Local checks** (no model call, an empty temp `CODEX_HOME`, the fake server
below):

- `-c mcp_servers.<s>.tools.<t>.approval_mode="bogus"` is rejected at load
  ("unknown variant `bogus`, expected one of `auto`, `prompt`, `writes`,
  `approve`"). So the per-tool key is real config.
- `-c mcp_servers.<x>.enabled=false` for a server that does not exist fails
  the whole load ("invalid transport in `mcp_servers.ghost`"). That is why
  the run reads `codex mcp list --json` first and only turns off servers
  that exist.
- `codex mcp add` takes names with letters, numbers, `-`, `_`, `:`, `@`, `/`
  and `.`, and rejects spaces. A hand-written config can hold any name.
- Quoted dotted keys in `-c` do not work:
  `-c 'mcp_servers."my.server".enabled=false'` fails with "invalid transport
  in `mcp_servers.\"my`" (single quotes too). Codex splits the key on every
  `.` and keeps the quotes as text. Names without a `.` work in the dotted
  form, even with `:`, `@`, `/`, a space, `"` or non-ASCII letters.
- An inline table, `-c 'mcp_servers={"my.server"={enabled=false}}'`, merges
  into the user's entry (`command`, `args`, `startup_timeout_sec` kept) and
  works for any name. Two separate `-c mcp_servers={...}` do not add up: the
  second replaces the first. So all server settings go in one argument.
- `codex debug prompt-input` (builds the prompt, no model call) starts the
  configured MCP servers. With `enabled=false` the fake never started (no
  process, no log line). Left on, it got `initialize` and `tools/list`.

**Real-CLI probes** (3, the maximum allowed). Each ran under a 120 s alarm,
from an empty temp folder, with `--ignore-user-config` so none of the user's
real MCP servers could load, signed in via ChatGPT, no skip-permissions flag.
The prompt was the one task from the 3-line sample ("Draft the release notes.
Owner: Priya. Due: Friday."), asking Codex to call `create_issue` once and to
list every tool name it can call. Servers: the tracker fake (tools
`create_issue`, `delete_issue`) and a fake named `other` (tool `other_tool`,
`enabled=false`), all in one inline-table `-c`, plus
`--disable apps --disable plugins`.

| # | Tracker entry | Result |
|---|---|---|
| 1 | name `tracker`, `enabled_tools=["create_issue"]`, `tools={create_issue={approval_mode="approve"}}` | exit 0 in 28 s. `tools/call create_issue` reached the fake; reply `TRACKER-1` and its URL. `other` never started. The only MCP tool the model listed was `mcp__tracker__create_issue` (no `delete_issue`, no `other_tool`, no connector tools). It also listed built-ins: `apply_patch`, `exec_command`, `write_stdin`, `view_image`, `image_gen__imagegen`, `web__run`, `list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource`. Banner still says `approval: never`. Working folder empty |
| 2 | name `trk.dot`, same per-tool entry | exit 0 in 25 s. The server started and got `tools/list`, never `tools/call`. No `trk.dot` tool in the model's list; reply `external_id` null |
| 3 | name `trk.dot`, `default_tools_approval_mode="approve"`, no `enabled_tools` | exit 0 in 23 s. Same as 2: no tool reached the model, reply null |

So a `.` in a server name hides its tools in Codex 0.152.1, whatever the
narrowing. The app refuses such a tracker up front.

The fake server, `fake_mcp.py`, was a ~60-line Python stdio MCP server
(newline-delimited JSON-RPC). It logs every request to a file in the temp
folder and answers `create_issue` with a fixed key and URL. It touched
nothing else.

**Nothing to clean up:** only local fake servers were called. No tracker,
connector or network service was called, and nothing was created or changed
outside this Mac.

## Known limits

- **Built-in tools are not closed off.** Shell (`exec_command`, inside the
  read-only sandbox), `apply_patch` (blocked by that sandbox), web search
  (`web__run`), image generation, `view_image` and sub-agent tools are still
  there. This ticket covers MCP servers only; closing these is a possible
  follow-up.
- **Skills still load.** Codex reads skills from `~/.agents/skills` even with
  an empty `CODEX_HOME` (seen in `prompt-input`).
- **A tracker that comes only from a plugin** is off along with plugins, so
  its sync shows as "not synced".
- **The server list is read just before the run.** If the user changes their
  Codex config in between and removes a server, the run fails with "invalid
  transport".
- **Codex upgrades may change how dotted names behave.** The dot refusal is
  based on 0.152.1; re-check it after an upgrade.

## Decisions taken

- **Per-tool approval only for a list of named tools.** For
  `mcp__<server>__*` (what the app always sends) the server-level
  `default_tools_approval_mode="approve"` is used, since there is no tool
  list to name.
- **One inline table for all servers**, because dotted keys break on names
  with a `.` and a second `-c mcp_servers={...}` replaces the first.
- **Fail early, before `exec`,** when the server list can't be read, the
  tracker is not in it, the tracker is turned off in Codex's config, or its
  name has a `.`.
- **`--disable plugins` as well as `--disable apps`.** Plugins can bring
  their own tools. `--disable <feature>` is in `codex exec --help` for
  0.152.1 ("Disable a feature (repeatable). Equivalent to
  `-c features.<name>=false`"). Asked Shann; answer: yes.
- **The fake `codex` in `src-tauri/src/sync/tests.rs`** only gains lines so it
  answers `mcp list --json`. Its test checks are unchanged. Asked Shann;
  answer: yes.

## Tests run

- `cargo test -p agent`: all pass (101 unit, 15 `codex_fake`, the rest of
  the integration files); the `#[ignore]`d real-CLI tests were not run.
- `cargo test -p meet-ai --lib sync::`: 16 pass, including
  `a_refused_codex_sync_exits_0_and_is_not_synced` and
  `a_reply_with_no_key_is_not_synced_and_writes_nothing` (the "not synced"
  check from TUR-11, verified, not redone).
- `cargo clippy -p agent --all-targets -- -D warnings`, `cargo fmt --all --check`: clean.
- `scripts/quality-gate.sh` on the changed files: pass (exit 0, not a timeout).
