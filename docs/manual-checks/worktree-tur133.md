# Manual checks: tur133

TUR-133 (D15): after Stop, while the last transcript lines are still being
written, the meeting view's notes card says *Finishing the transcript…*
(new run state `waiting-for-transcript`) instead of nothing. Tested headless:
`after_stop` with a recording sink (Waiting then Running, Waiting then the
not-final failure, Waiting then Idle when notes are switched off mid-wait) and
the `NotesRun` vitest (no button in the waiting state).

## Run by hand

1. With `agent.auto_run` on and an agent set up, record a short call, press
   Stop and open the meeting at once.
   Expect: the notes card shows "Finishing the transcript…" and "Notes start
   on their own once the last lines are written.", with no button; then
   *Writing notes…* with Cancel, then the notes.
   Why skipped: needs a signed build, a mic and the running app.
2. Same, but switch notes off for the meeting while it says *Finishing the
   transcript…*.
   Expect: the card goes away (no failure shown).
   Why skipped: as above.

## Out of scope

- Manual-notes mode (`agent.auto_run` off) pressing "Make notes now" before
  the transcript is final is not handled here: Stop starts no wait in that
  mode, so there is no waiting state to show.
