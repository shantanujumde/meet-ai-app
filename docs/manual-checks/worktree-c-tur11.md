# Manual checks: TUR-11 (Sync tasks to Linear / Jira / GitHub)

Branch `worktree-c-tur11`. Everything below needs a signed-in agent CLI, a
real tracker or the running app, so none of it ran in this branch. The
automated side is covered by fake-harness tests (`cargo test -p meet-ai --lib
sync`, `cargo test -p agent mcp`, `cargo test -p prompts push_ticket`, and the
vitest files for `SyncButton`, `MeetingTasks` and `TrackerSettings`).

No real sync was run against Linear or any tracker, on purpose.

## 1. Sync one task to the Turing team in Linear with Claude Code

Why skipped: needs the running app, a signed-in `claude` and a real Linear
workspace.

1. In Terminal, check the Linear connector loads outside a project:
   `cd "$(mktemp -d)" && claude mcp list`. Expect a line like
   `claude.ai Linear: https://mcp.linear.app/mcp - ✔ Connected`. (Seen on
   2026-10-02 on this Mac.)
2. Make the prompt pick the team: copy
   `crates/prompts/templates/push-ticket.md` to
   `~/Meetings/.app/prompts/push-ticket.md` and add a line such as "Create
   the issue in the Turing team." under "## The issue". (The built-in prompt
   says to use the default team, or the only one.)
3. Open the app, Settings, Tracker: tracker Linear, server `claude.ai Linear`
   (pick it from the list, which shows "Connected"). Save.
4. Open a meeting with tasks and press **Sync** on one task.
5. Expected: "Syncing…", then the button becomes **Open in Linear** with the
   key (e.g. `TUR-123`). The ticket file gains `synced_to: linear`,
   `external_id: TUR-123`, `external_url: https://linear.app/...`. Open in
   Linear opens the issue in the browser. The issue is in the Turing team,
   with the task's title, details, owner/due and a line naming the meeting.
6. Press **Sync all** on a meeting with two or more unsynced tasks: they sync
   one after the other; the synced one from step 4 is skipped.

## 2. Same with Codex: expected to FAIL until TUR-16

Why skipped: needs a signed-in Codex with a Linear MCP server in
`~/.codex/config.toml`.

TUR-5 measured that `codex exec` refuses every MCP tool call
(`approval: never`) and still exits 0. TUR-11 does not pre-approve the
tracker's tools; that is TUR-16. So today:

1. Settings, Tracker: server = the Codex config name (e.g. `linear`). The
   list comes from `codex mcp list --json` and shows "Set up" (Codex does not
   health-check).
2. Press **Sync**. Expected today: an error under the button, "The agent
   finished but did not create an issue in Linear. Check that "linear" is
   connected and signed in (Settings, Tracker), then press Retry.", and the
   ticket file unchanged (no `synced_to`). That is the intended "not synced"
   path (tested with a fake `codex` in
   `a_refused_codex_sync_exits_0_and_is_not_synced`).
3. After TUR-16 lands, repeat: expect the same result as check 1.

## 3. Settings, Tracker list on the real CLIs

Why skipped: running the app.

- Claude Code: the list should match `claude mcp list` run in an empty temp
  folder. It health-checks every server, so it can take up to a minute; the
  screen shows "Checking…" meanwhile.
- Unchecked: whether `claude mcp list` exits non-zero when one server fails
  its health check. If it does, the whole list shows as an error instead of a
  partial list (`agent::mcp::claude_servers` treats a non-zero exit as
  `CliFailed`). Run it with one broken user-scoped server and look at `$?`.
- A server added with `claude mcp add` without `--scope user` (project
  scope) must NOT appear, since runs start in a temp folder. The Settings tip
  says to use `claude mcp add --scope user …`.

## 4. Cancel and time limit

Why skipped: running app plus real CLI.

Press Sync, then Cancel while "Syncing…": the button goes back to Sync with
no error and nothing is written. With `agent.timeout_sec` set to 5 in
`config.jsonc`, a sync stops with "the agent did not finish within 5
seconds".

## Real-CLI probes made in this branch

Two, both read-only listings with no model call and no prompt:
`codex mcp list --json` and `claude mcp list`, each in an empty temp folder,
to record the output formats the parsers read.
