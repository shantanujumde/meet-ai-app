# Manual checks: TUR-154

Ticket numbers are global: a notes run, a hand-made ticket and the copied
wrap-up prompt all use `store::agent_notes::next_ticket_number` (every ticket
file plus every number any meeting's `agent_tickets` or `retired_tickets`
names). The index `tickets` table is keyed by (meeting, ticket) with
`SCHEMA_VERSION` 2, so an existing `index.db` is rebuilt from the markdown on
the next open. `SyncRuns` and retention's `syncing_in` are keyed by (meeting,
ticket). All of this is unit-tested headless; nothing below was run here.

## Run by hand

1. Open the app on a meetings folder indexed by an older build.
   Expect: search still works; `.app/index.db` was rebuilt once (its
   `user_version` is 2).
   Why skipped: needs the running app.
2. Meeting A has notes with tickets up to TICK-0010. Delete TICK-0010's file,
   then run notes on meeting B.
   Expect: B's first ticket is TICK-0011, never TICK-0010.
   Why skipped: needs the running app and a signed-in agent CLI.
3. Copy the wrap-up prompt for meeting B while meeting A has TICK-0004.
   Expect: the prompt names TICK-0005 (or higher) as the first file, not
   TICK-0001.
   Why skipped: needs the running app.
4. Two meetings that both hold a TICK-0001 (copy a meeting folder): press Sync
   on both at once.
   Expect: both runs start; neither says "already being synced".
   Why skipped: needs the running app, a signed-in agent CLI and a tracker.
