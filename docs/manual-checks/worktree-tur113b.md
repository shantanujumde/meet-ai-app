# Manual checks: tur113b

TUR-113b: the Rust half of "Tickets sync on their own" (SPEC A28). A queue
(`src-tauri/src/sync/auto.rs`) sends each new root ticket to the tracker in
the background, one at a time, through the same agent run as a Sync press.
It starts on Approve, Approve all, a hand-made ticket, Retry
(`retry_ticket_sync`) and a save in Settings, Tracker. Status goes out on
`TICKET_SYNC_EVENT` (`ticket-sync://status`); `ticket_sync_states` gives the
current states and `trackerSetUp`. `send_test_ticket` is the read-only
"Send a test ticket" check. No UI here: TUR-113c adds the Tickets statuses,
Retry and the test button. Everything below was tested with a fake `claude`
(`test_support::FakeCli`) and `FakeHarness` only.

Until TUR-113c ships, call the commands from the webview's dev console in a
dev build (`__TAURI_INTERNALS__.invoke(...)`) and listen with
`__TAURI_INTERNALS__.transformCallback` or the app's log.

## Run by hand

1. Fresh `config.jsonc` with no `tickets` section, Claude Code signed in with
   the claude.ai Linear connection. Approve a suggested task.
   Expect: nothing is sent; `ticket_sync_states` says `trackerSetUp: false`;
   no Linear issue appears.
   Why skipped: needs the running app and a signed-in Claude Code.
2. Save Settings, Tracker (Linear, "claude.ai Linear"), or call
   `set_tracker`.
   Expect: every ticket in `~/Meetings/tickets/` with no issue yet (not
   `dropped`) goes to Linear one after another; each shows `queued`,
   `sending`, then `sent` on `ticket-sync://status`, and its file gains
   `synced_to`, `external_id`, `external_url`. The Linear issue's body holds
   the ticket's description word for word.
   Why skipped: needs a real Linear account and MCP connection.
3. Approve two tasks from a meeting, Discard one; make one ticket by hand.
   Expect: the three tickets are each sent once, in order, never two agent
   runs at the same time (`ps` shows one `claude -p` at a time).
   Why skipped: needs the running app and a real recording.
4. Disconnect the Linear connection in Claude Code, then approve a task.
   Expect: `failed` with kind `sync-unreachable` and "Couldn't send to
   Linear: your agent couldn't reach Linear. Check that "claude.ai Linear"
   is connected and signed in in Claude Code, then press Retry." Approve
   another task: only the new one runs. Reconnect, call
   `retry_ticket_sync` on the first: it is sent. Quit and relaunch with a
   failed ticket: nothing runs until Retry or a Tracker save.
   Why skipped: needs real accounts.
5. `send_test_ticket` with `tracker: "linear"`, `trackerMcp: "claude.ai
   Linear"`.
   Expect: "Claude Code reached Linear. New tickets will go to <team>." and
   no new issue, comment or change in Linear (check its activity log). With
   the connection signed out: `sync-unreachable` ending "then send the test
   ticket again." Verify it: that the agent keeps to a read call depends on
   the model following the prompt; the CLI allows every tool of that server.
   Why skipped: needs real accounts.
6. Codex as the agent: repeat 2 and 5.
   Expect: the same results. Verify it: TUR-5 measured that a refused Codex
   MCP call exits 0 with no key; the check's reply then reads as
   `sync-unreachable`.
   Why skipped: needs a signed-in Codex.

## Known limits

- Failures are remembered in memory only (by design, SPEC A28): after a
  restart a failed ticket shows as not sent until Retry or a Tracker save.
- A `config.jsonc` that does not parse fails the queued ticket once with the
  config error, not remembered under any settings, so the next trigger tries
  it again.
- The existing meeting-page Sync button (`sync_task`) still works next to the
  queue; a ticket a press is sending is skipped by the queue (`sync-busy`) and
  reports through the press. TUR-113c removes that button.
