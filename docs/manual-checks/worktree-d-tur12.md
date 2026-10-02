# Manual checks: d-tur12

TUR-12: the per-meeting "Make notes for this meeting" switch (SPEC A11, "Skip
one meeting"). It sits in the meeting's header, so it shows while the meeting
records and after. Off writes `agent_notes: "off"` into that meeting's
`meeting.md` (`store::notes_switch::set`), creating the file if there is none
yet; on removes the key. The notes run reads the key before it looks for the
agent CLI, so a meeting switched off mid-recording sends nothing at Stop.
Switching off while a run is going cancels it, and the run ends as
"notes off". Switched back on, a meeting with no notes offers **Make notes
now**. The sidebar and the Meetings page mark switched-off meetings
"Notes off". While notes are off, Copy prompt is hidden too.

Tested headless: store tests for the file itself (including a deleted
`index.db` and a fresh rescan), `agent_run` tests with the fake harness (off
before Stop is never sent, off then on writes notes, off mid-run writes
nothing, the switch cancels a running run), a `list_in` test for the marker,
and vitest against a mocked IPC for the switch, Make notes now and the list
marker.

## Run by hand

1. **Off mid-recording.** Signed in to Claude Code, `agent` at its defaults,
   record a short meeting (`just bundle-signed`, open the app). While it
   records, open the meeting and turn "Make notes for this meeting" off.
   Press Stop.
   Expect: no *Writing notes…*; no `claude` process starts
   (`pgrep -fl claude` stays empty); `meeting.md` has `agent_notes: "off"`
   and no notes; the sidebar row says "Notes off".
   Why skipped: needs the mic, the signed bundle and a signed-in CLI.
2. **Back on, Make notes now.** Same meeting as 1, turn the switch on.
   Expect: "Notes are on for this meeting" with **Make notes now**; pressing
   it shows *Writing notes…*, then the notes; the "Notes off" marker is gone.
   Why skipped: needs the running app and a signed-in CLI.
3. **Off while the notes are being written.** Record and stop with the switch
   on; while it says *Writing notes…*, turn the switch off.
   Expect: the run stops, no notes or tickets are written, the notes section
   disappears, no `claude` process is left.
   Why skipped: needs the running app and a real CLI.
4. **Survives a relaunch.** Turn notes off for a meeting, quit, delete
   `~/Meetings/.app/index.db`, reopen.
   Expect: the meeting still says "Notes off" in the list and the switch is
   off when opened.
   Why skipped: needs the running app.
5. **Looks right.** Check the switch in light and dark mode and with
   Increase Contrast on, and the sidebar row with a long title, "● Recording"
   and "Notes off" together at the narrowest window width.
   Why skipped: needs the running app's window.

## Decisions taken without an answer

- The file says `agent_notes: "off"` (quoted). The YAML writer quotes it so
  YAML 1.1 readers do not read it as `false`. meet-ai reads `off`, `"off"`
  and `false` all as off, so a hand-written bare `off` works too.
- `hasAnalysis` ("Wrapped up") now needs `analyzed_by` or text in one of the
  four sections, not just a `meeting.md` file, because switching off can
  create a `meeting.md` with no notes in it.
- Copy prompt is hidden while notes are off: its hint says to paste the
  transcript into an agent, which is what the switch says not to do.
- A run cancelled by switching off ends as "notes off", not "cancelled".
  Pressing Cancel still ends as "cancelled".
