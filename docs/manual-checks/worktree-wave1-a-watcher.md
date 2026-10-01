# Manual checks: TUR-100 folder watcher

None of these ran headless: they need the running app and a real meetings folder.

1. **External edit shows up.** Run the app, open the list. In another editor, change
   the `title` in some `meeting.md` and save. Expect: within about a second the
   list shows the new title. The selection and the open page do not move.
   **Passed 2026-10-01:** new title in the list within 1.5 s; selection and open page unchanged.
2. **Own notes never bounce.** Open a meeting, type in the notes pane for 10 seconds.
   Expect: no list refresh flicker, the cursor never jumps. (Saves are marked as
   our own for 750 ms; the watcher waits 500 ms.)
   **Passed 2026-10-01:** 21 words typed over about 10 s; `notes.md` matched exactly, no jump.
3. **Agent write is not hidden.** While the app is open, run
   `echo hi >> <meeting>/notes.md` in a terminal. Expect: the list refreshes (an
   outside write to a file the app also writes must still be seen).
   **Passed 2026-10-01:** a line appended to another meeting's `transcript.md` moved its count from 3 to 4 lines.
   **🐞 But an outside write to the open meeting's `notes.md` is lost.** The notes pane does not reload it, and the next keystroke saves the pane over the file: an appended "hi from outside" was gone one keystroke later. An agent writing notes while the user has that meeting open loses its text.
4. **Folder change restarts the watcher.** Settings > change folder, move to a new
   empty folder. Then edit a `meeting.md` in the NEW folder. Expect: list refreshes.
   Edit one in the OLD folder: no refresh.
   **Passed 2026-10-01** for the new folder: a title edit there showed within 1.5 s. The old-folder half does not apply, because the move deletes the old folder.
5. **Folder that does not exist at launch.** Start with a meetings folder that
   does not exist yet, then record once. Known gap: the watcher starts only at launch
   and on folder change, so external edits are not noticed until one of those
   happens. Expected today: no refresh until restart.
   **Confirmed 2026-10-01:** launched with `MEET_AI_MEETINGS_ROOT` pointing at a missing folder, recorded once, then changed the title outside: the list kept showing "Meeting".
6. **During a recording** the recorder writes many files in the folder, so the list
   refreshes now and then (silent, no spinner). Expect: no selection or cursor
   movement. Why skipped: needs mic and system audio.
   **Passed 2026-10-01** over 4 recordings: the list never jumped, flickered or lost the selection.

Decision taken without an answer from Shann: kept the existing constant name
`SELF_WRITE_SUPPRESSION` (750 ms) rather than renaming it to `SELF_WRITE_PAUSE`.
