# Manual checks: tur113a

TUR-113a: the Rust half of "approve tasks from a meeting into Tickets". A
meeting's tasks are suggestions in `<meeting>/tickets/`; Approve moves one to
`<root>/tickets/`, Discard deletes it and retires its number, and at launch the
tickets that were already synced move to Tickets (SPEC A25). Sync errors now
have one kind per cause (`sync-unreachable`, `sync-refused`, the `agent-*`
kinds with new words). New commands: `approve_task`, `approve_all_tasks`,
`discard_task`; `TicketSummary` gains `suggested`, `owner`, `due`,
`meetingTitle`. No UI here: the buttons, the Tickets hint and the error layout
are TUR-113c, auto-sync and the test ticket TUR-113b. Everything below was
tested with fake folders and fake agents only.

## Run by hand

1. With a build of this branch, open a meetings folder that already has a
   meeting with tasks in `<meeting>/tickets/`, one of them with `synced_to:
   linear` (a ticket synced before this change). Launch the app once.
   Expect: that ticket is now in `~/Meetings/tickets/` with the same `id`,
   `meeting:` and `external_id`; the others are still in the meeting folder;
   `.app/logs/meet-ai.log` has one "moved a synced ticket from its meeting to
   Tickets" line naming it. Quit and launch again: no new line, nothing moves.
   Why skipped: needs the running, signed app and a real meetings folder.
2. Record a short meeting with a few clear tasks and let the notes run finish.
   Expect: every ticket file in `<meeting>/tickets/` has a description of 1 to
   3 sentences under the frontmatter, not an empty body.
   Why skipped: needs a real recording and a signed-in Claude Code or Codex;
   the prompt change is checked by its snapshot only.
3. Until TUR-113c ships the buttons, call the commands from the webview's dev
   console in a dev build:
   `__TAURI_INTERNALS__.invoke("approve_task", { meetingId: "<id>", ticketId: "TICK-0001" })`,
   then `"discard_task"` on another, then `"approve_all_tasks"`.
   Expect: the approved file moves to `~/Meetings/tickets/`, the discarded one
   is gone and `meeting.md` gains `retired_tickets: [TICK-…]` and loses its
   Action Items line; a new hand-made ticket gets a number above the
   discarded one.
   Why skipped: needs the running app.
4. With Linear set up (Settings, Tracker, "claude.ai Linear"), sync an
   approved ticket that has a description.
   Expect: the Linear issue's description holds the ticket's text word for
   word, then the owner and due date, then the "came from meet-ai task" line.
   Why skipped: needs a signed-in Claude Code with the Linear connection.
5. In Claude Code, disconnect the claude.ai Linear connection, then sync a
   ticket.
   Expect: kind `sync-unreachable`, text "Couldn't send to Linear: your agent
   couldn't reach Linear. Check that "claude.ai Linear" is connected and
   signed in in Claude Code, then press Retry."
   Then reconnect it and set the tracker project to one you cannot write to.
   Expect: kind `sync-refused`, "Linear refused the ticket: <reason>. Check
   the project in Settings, Tracker, then press Retry." Verify it: whether
   the agent passes a usable `refused_reason` depends on what the Linear MCP
   server returns, which was not measured here.
   Why skipped: needs real accounts and a real MCP server.
6. Run `claude auth logout`, then sync a ticket.
   Expect: kind `agent-not-signed-in`, "Couldn't send: Claude Code isn't
   signed in. Open a terminal, run `claude auth login`, sign in, then press
   Retry."
   Why skipped: needs a real Claude Code account. The command is the one
   Settings, Notes already shows (`agent_setup::view`), used instead of the
   ticket's "run `claude`" so both screens say the same thing.

## Known limits

- The `refused_reason` key is required in the sync reply schema (Codex's
  strict schemas need every key). A user's own `push-ticket.md` saved before
  this change does not mention it; the CLIs are given the schema, so the reply
  should still carry it, but a reply without it fails as `agent-bad-reply`.
  Verify it with a saved old template on Claude Code and on Codex.
- Approve all skips a ticket file with broken frontmatter (logged at warn) and
  approves the rest; that file stays a suggestion with its "needs attention"
  badge.
