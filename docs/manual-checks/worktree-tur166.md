# Manual checks: TUR-166

Cheaper meeting list, search index and ticket numbering. Sidebar no-flash
(Vitest), the list's summary cache, FTS deletes by rowid (query plan), the
recording throttle, the ticket-number cache and the split `meeting.md` lock are
unit-tested headless. Nothing below was run here: each needs the running app.

## Run by hand

1. Open a meeting with a long sidebar list, scroll the list down, type in the
   notes pane and pause.
   Expect: the sidebar keeps its rows and scroll position; no "Reading your
   meetings folder…" flash after each autosave, rename or notes run.
   Why skipped: needs the running app (jsdom has no layout or scroll).
2. With a few hundred meetings, edit one meeting's `notes.md` in another
   editor.
   Expect: the list updates within a second; the log shows no slowdown. Only
   that folder's three files are read (the rest are only `stat`ed).
   Why skipped: needs the running app and a large meetings folder.
3. Record a 10-minute meeting and search for a word said a minute ago.
   Expect: found within about 10 s of being said (the live meeting is indexed
   at most every 10 s); after Stop, every line is searchable.
   Why skipped: needs a signed build, a mic and a meeting.
4. Start the app over an `index.db` from v0.4/v0.5.
   Expect: it is rebuilt once (schema 3) and search answers as before.
   Why skipped: needs the running app.
5. Rename a meeting while a notes run is writing tickets.
   Expect: the rename lands without waiting for the run to finish numbering.
   Why skipped: needs the running app and an agent CLI.
6. Windows and Linux: make a ticket by hand in one meeting, then a second.
   Expect: consecutive numbers (the cache relies on a folder's modified time
   changing when a file is added; verify it on NTFS and ext4).
   Why skipped: needs a real Windows / Linux machine; CI runs the unit tests.

## Known

- TUR-152's finding (searching "integration" finds nothing in the
  `2026-09-02-1000-retro` fixture) is not an index bug: the only line with
  that word is `[00:00:09] Priya: …`, a speaker SPEC §3.4 does not parse,
  which the fixture has on purpose. The meeting view leaves it out too. A test
  now pins this.
