# Manual checks: TUR-153

Tickets screen refresh. Decided with the manager (A1): the listing half of the
ticket is superseded by TUR-113 / SPEC A26. A notes run's tasks are
suggestions in `<meeting>/tickets/` and stay off Tickets until approved, so
`tickets::list_in` stays root-only (now pinned by a Rust test over a shared
folder and two meeting folders). Tickets already subscribed to
`onMeetingsChanged`; this change adds a latest-wins guard so an older read
landing last cannot bring back a stale list, plus Vitest tests for the quiet
reload. Nothing below was run here: each needs the running app.

## Run by hand

1. Open Tickets. In a second window (or from the meeting page), approve a
   suggested task on a meeting's page.
   Expect: the task appears on Tickets, with "From: <meeting>", without
   navigating away and back; no "Reading your tickets…" flash; the scroll
   position stays where it was.
   Why skipped: needs the running app.
2. With Tickets open, let a notes run finish for a meeting.
   Expect: Tickets does not list its tasks (they are suggestions, SPEC A26);
   the meeting's page lists them with Approve / Discard.
   Why skipped: needs the running app and a signed-in agent CLI.
3. With a tracker set up and Tickets open, let a send write an issue key.
   Expect: the card shows the key without navigating.
   Why skipped: needs the running app and a tracker account.
4. Add a ticket by hand with New ticket, then edit a file in
   `<root>/tickets/` in an editor.
   Expect: the new ticket stays listed first; the edit shows after the
   folder-change event.
   Why skipped: needs the running app.
