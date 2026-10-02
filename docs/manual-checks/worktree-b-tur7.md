# Manual checks: b-tur7 (TUR-7, app writes notes and tasks from the agent's JSON)

`crates/store`: `store::agent_notes::write` turns a checked
`prompts::notes::Notes` into the four `meeting.md` sections and one
`tickets/TICK-NNNN.md` per task. Everything here is covered by headless tests
on JSON fixtures (`crates/store/tests/agent_notes.rs`,
`crates/store/tests/fixtures/notes/`). Nothing in the app calls it yet; the
notes run that will (A11 "Call ends") is a later ticket.

## Run by hand

1. **The watcher stays quiet for these writes in the running app.** Once a
   notes run calls `agent_notes::write` with the app's `SelfWrites` (the one
   `MeetingsWatch` holds), finish a meeting with the meeting view open.
   Expect: the view refreshes once from the notes-run result, not a second
   time from a watcher echo of `meeting.md` or the new tickets.
   Why skipped: needs the running app and a call site that does not exist yet.
   The unit side (each written and removed path is noted) is tested.
2. **A real agent's answer lands as files.** Run the A11 notes command on the
   3-line sample, feed `structured_output` to `Notes::from_value`, then to
   `agent_notes::write` against a copy of a meeting folder.
   Expect: four sections filled, `analyzed_by: claude-code`, one ticket per
   task starting at the next free number.
   Why skipped: needs a signed-in CLI. Not probed in this run; the JSON
   fixtures stand in for it.

## Choices made without a ruling

- **"Store's existing lock".** `store` had no lock. `agent_notes` adds a
  process-wide `Mutex` held from the ticket-number scan to the last write.
  The hand-made ticket path (`src-tauri/src/tickets.rs`, outside this ticket)
  does not take it; it scans only `<root>/tickets/` and retries on an
  existing file, so in theory it can pick a number a meeting already uses.
  Worth moving its allocation into `store` behind the same lock.
- **Numbering scope.** Highest `TICK-NNNN` file name (plus one) across every
  meeting's `tickets/` **and** the root's `tickets/` (hand-made tickets),
  judged by file name so a broken ticket still holds its number.
- **"Unchanged since the app wrote it".** `meeting.md` gets an
  `agent_tickets` map, ticket id → SHA-256 of the file as written. A ticket is
  replaced only when its bytes still hash to that, `status` is `open`, and
  `synced_to`, `external_id` and `external_url` are all empty. A touched
  ticket is kept and dropped from the map, so a later run never claims it back
  even if the user reverts the edit. A ticket the user deleted is not
  recreated.
- **Replace keeps the number.** Untouched tickets are reused in id order for
  the new tasks; extra tasks get fresh numbers; untouched tickets with no task
  left are deleted. No matching of old to new tasks by title (the ticket says
  exactly this rule), so a kept, edited task can appear again as a new one.
- **Text format follows the copy-prompt template** (`wrap-up.md`):
  `- TICK-0001: Title (Owner, due Friday)` in Action Items, `None.` under an
  empty heading, and the due date in the ticket body (`Due: Friday.`), since
  §3.3 has no due field. Kept tickets are listed after the new ones.
- **A summary line starting with `## `** is written as `\## ` so it cannot
  split the section on the next read. Bullet items are folded onto one line.
- **`analyzed_at` is passed in** by the caller (ISO-8601 with offset), so
  `store` stays free of a clock.
- **Write order.** Tickets first, `meeting.md` last. A broken or non-UTF-8
  `meeting.md` is refused before any ticket is written.
