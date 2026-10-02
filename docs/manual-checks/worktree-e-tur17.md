# Manual checks: e-tur17 (TUR-17, notes run after Stop)

`src-tauri/src/agent_run.rs` and `agent_run/`: Stop now waits for
`transcript.md` to be final before a notes run starts, starts no run at all
when notes are off, and saves the notes into the meetings folder as it is when
the answer comes back. `src-tauri/src/live_transcript/mod.rs`: the
`TranscriptFinal` signal. `src-tauri/src/folder_move.rs`: `writing_in_root`,
the helper TUR-20 (sync) is meant to reuse.

The headless tests cover all of this with a fake agent CLI and fake engines.
What follows needs the running app, a microphone and a signed-in agent.

## Run by hand

1. **A normal call still gets notes.** `agent.auto_run` on, Claude Code signed
   in. Record a short call, press Stop.
   Expect: *Writing notes…* shows within a second or two, then the notes. The
   last thing said before Stop is in `transcript.md` and the summary can use it.
   Why skipped: needs the running app, mic audio and a signed-in CLI.
2. **Notes off: nothing at all.** Switch *Make notes for this meeting* off
   during a recording, then press Stop.
   Expect: no *Writing notes…*, not even for a moment; no `claude` process
   starts (check Activity Monitor); the meeting view offers *Make notes now*.
   Why skipped: needs the running app and a real recording.
3. **Slow engine.** Hard to trigger on purpose. If a Stop ever shows "Live
   transcription did not finish within 10 seconds", expect: the notes run
   starts only after the transcript stops growing, not at the 10-second mark.
   If it never finishes within 2 minutes after that, the meeting view shows
   "The transcript was still being saved, so no notes were written…" with
   Retry, and no agent was started.
   Why skipped: needs a real engine that is slow to flush.
4. **Folder moved while notes are being written.** Start a notes run (Make
   notes now), and while it shows *Writing notes…*, change the meetings folder
   in Settings.
   Expect: the move goes through, and the notes land in the meeting in the new
   folder, not in a recreated old one.
   Why skipped: needs the running app and a signed-in CLI that takes long
   enough to answer.

## Known limits, not changed here

- While Stop waits for a slow transcript, the meeting view shows the meeting
  as having no run yet, so *Make notes now* is pressable and would run on the
  transcript as it is at that moment. Showing a waiting state would need a new
  run state and a UI change; out of scope for this ticket.
- A save that lands exactly while a folder move is running is refused
  ("meet-ai is moving your meetings folder"), shown as "could not be saved"
  with Retry. Retry after the move writes the notes.
- The "not final" failure reuses the `could-not-start` kind, so no new kind
  was added to the meeting view or the generated bindings.
