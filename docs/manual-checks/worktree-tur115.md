# Manual checks: TUR-115 (meeting screen scrolls sideways)

jsdom has no layout, so the fix is covered by class/rule tests
(`src/test/noSidewaysScroll.test.ts`, `src/ui/NotesRun.test.tsx`). These need
the running app.

## 1. No sideways scroll on a long meeting

- Run: open "Callback Automation Review" (or any meeting whose transcript or
  notes contain a long URL or token with no spaces). Swipe sideways on the
  trackpad over the main column.
- Expected: nothing moves. "Summary", "Decisions", "Action Items", ticket
  lines ("TICK-0006: …") and the top cards start at the normal left margin;
  no empty strip on the right. Long URLs wrap inside their line.
- Why skipped: needs the running app and a real meeting.

## 2. Live transcript wraps too

- Run: record, say or paste-play audio producing a long token in the live
  transcript.
- Expected: the token wraps inside the text column; no sideways scroll.
- Why skipped: needs a signed build and a mic.

## 3. Nothing silently hidden (for TUR-112)

- Run: on the same meeting, check the task rows / sync cards at the top
  (MeetingTasks, SyncButton) with long titles.
- Expected: their text wraps and is fully visible. `.content` now has
  `overflow-x: clip`, so if one of those rows is the real wide element it
  would be cut at the right edge instead of scrolling; if so, that row needs
  `min-w-0`/`wrap-anywhere` in TUR-112's files (not touched here).
- Why skipped: the wide element is not proven without layout.
