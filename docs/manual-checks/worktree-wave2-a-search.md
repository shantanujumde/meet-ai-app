# Manual checks: TUR-101 search

Skipped here because they need the running app and a window.

1. **Search finds a real meeting.** Run the app against a folder with a few
   meetings (`MEET_AI_MEETINGS_ROOT=<folder>`). Type a word from a transcript
   into the search box above the meetings list. Expected: the list is replaced
   by results with title, date, a snippet with the word highlighted and a
   timestamp; clicking one opens that meeting; clearing the box brings the
   normal list back.
2. **Delete the index while the app is closed.** Quit the app, delete
   `<meetings folder>/.app/index.db` (and any `index.db-wal` / `-shm`), start the
   app, search the same word. Expected: the same results, after a short wait on
   the first search if the folder is large.
3. **Edits show up without a restart.** With the app open, edit a `notes.md` in
   another editor and add a new word. Search for it a second later. Expected:
   the meeting is found.
4. **Folder pick.** Change the meetings folder in Settings, then search.
   Expected: results come from the new folder only.

## Results, 2026-10-01 (signed bundle, a throwaway meetings folder, driven by script)

- 1 ✅ "Priya" finds the transcript line with the word highlighted and its timestamp; clicking it opens the meeting; clearing the box brings the list back.
- 2 ✅ With `index.db`, `-wal` and `-shm` deleted while the app was closed, the same search returns the same result after relaunch.
- 3 ✅ A word appended to a `notes.md` while the app ran is found about 1.5 s later.
- 4 ✅ After moving the folder, results come from the new folder (the page header shows the new path).

Bugs found:
- A meeting with no `meeting.md` shows in results as its folder id (`2026-10-01-2132-meeting`) with "No date", while the list shows "Meeting · Today 21:32".
- A meeting with a `meeting.md` shows its raw date (`2026-10-01T21:50:00+05:30`) instead of a formatted one.
- Ticket text is not searched (see `worktree-wave1-b-tickets.md`).
- There is no way back to the meetings page, where the search box is, from Tickets, Settings or a meeting: the sidebar has no link to it, ⌘[ does nothing, and Review only links back on an error. Only a relaunch gets there.

Decision taken without an answer from the manager: search results include notes,
meeting.md sections and ticket text as well as transcript lines (those hits have
no timestamp).
