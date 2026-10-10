# Manual checks: TUR-152

Search now sees the app's own writes (`save_notes` refreshes the meeting in
the index; the notes run already did since TUR-107), a kept `index.db` is
caught up with the folder at launch (`Index::catch_up`, run from
`SearchIndex::warm` on its own thread, logging `changed` at info), and
`Index::update` matches paths under a symlinked root's real path. The store
side is covered headless by `crates/store/tests/index_catch_up.rs` and
`index_symlink.rs`. Nothing below was run here: each needs the running app.

## Run by hand

1. Open a meeting, type a word nobody said (say "kangaroo") in the notes pane,
   wait for the autosave, then search for it.
   Expect: the meeting is found, with no timestamp.
   Why skipped: needs the running app.
2. Run the notes agent on a meeting, then search for a word from its summary,
   a decision and a ticket title. Start a meeting with the agent's title.
   Expect: all three are found; the pre-meeting brief names the earlier
   meeting.
   Why skipped: needs the running app and a signed-in agent CLI.
3. Quit meet-ai. In Finder (or a terminal) edit one meeting's `notes.md`, add
   a new meeting folder with a `notes.md`, and delete another meeting folder.
   Launch meet-ai and search.
   Expect: the edit and the new meeting are found, the deleted one is gone,
   and the log has `search index caught up with the meetings folder
   changed=3`. A second launch logs `changed=0`.
   Why skipped: needs the running app.
4. Point the meetings folder at a symlink (`ln -s <real folder> ~/MeetingsLink`
   and choose it, or an iCloud/Dropbox folder linked into place). Edit a
   `notes.md` in an editor while meet-ai is open, then search.
   Expect: the edit is found within a couple of seconds.
   Why skipped: needs the running app.
5. Windows: delete a meeting's `notes.md` (not its newest file) while meet-ai
   is closed, then launch and search for a word only in those notes.
   Expect: not found. Relies on NTFS moving a folder's modified time when an
   entry is removed; verify it.
   Why skipped: needs a real Windows machine (CI runs the same store test).

## Known

- The first launch after this change reads every meeting once: the index's
  per-meeting time now includes the folder's own modified time, so every
  stored time differs once. Later launches only read what changed.
- `tests/fixtures/meetings/2026-09-02-1000-retro/transcript.md` lines are not
  in the index (searching "integration" finds nothing), so the new tests probe
  that meeting with a word from its `meeting.md`. Not looked into; outside
  this ticket.
