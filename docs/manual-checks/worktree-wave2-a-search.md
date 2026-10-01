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

Decision taken without an answer from the manager: search results include notes,
meeting.md sections and ticket text as well as transcript lines (those hits have
no timestamp).
