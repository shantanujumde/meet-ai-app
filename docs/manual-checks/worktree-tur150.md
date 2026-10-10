# Manual checks: tur150

TUR-150: the notes pane reads the notes on disk once, when it opens, and
never puts them back over what is typed (`src/ui/NotesPane.tsx`; the reset
effect on `initialNotes` is gone, and `Review` renders it with
`key={summary.id}`). The meeting screen is keyed on the meeting id
(`<MeetingReview key={id} id={id} />`), so switching meetings starts a fresh
screen, and its `load` and `MeetingTasks.load` show only the newest read's
answer (a read that started earlier and lands later is dropped).

Tested headless on a Mac:

- `pnpm vitest run src/ui/NotesPane.test.tsx src/routes/Review.test.tsx src/ui/MeetingTasks.test.tsx`
  (47 tests): typing, autosave, then more typing while the parent re-reads
  the meeting keeps and saves the newer text; going away with a save still
  waiting writes it; another meeting gets its own notes and the last one's
  waiting text goes to the last one; on the Review screen, notes typed while
  the recording stops survive the re-read and are saved; switching from
  meeting A to B with A's read answering last shows B (title, transcript,
  notes) and Rename renames B; a re-read that started first and lands last
  does not replace the newer one; the same for suggested tasks.
- Each new test fails on the code before this change (checked by stashing
  the three source files).

Nothing below ran: no app was launched.

## Run by hand

Use a build of this branch.

1. Record a meeting and open its page. Type "abc" in Notes, wait a second,
   then type "def" and at once stop with the global shortcut.
   Expect: Notes still says "abcdef" after the transcript appears, and
   `notes.md` in the meeting folder ends with "abcdef".
   Why skipped: needs the running app, a mic and the global shortcut.
2. With many meetings in the list, click one meeting and then another
   quickly, several times.
   Expect: the page always shows the meeting that is highlighted in the
   list; renaming its title renames that meeting.
   Why skipped: needs the running app and real disk reads with real timing.
3. Switch between two meetings with the list.
   Expect: "Opening this meeting…" shows only for a moment, if at all, while
   the next meeting loads (the screen now starts fresh for each meeting
   instead of leaving the last one up). Note it if it flashes in a way that
   looks broken.
   Why skipped: jsdom has no layout or real timing.
