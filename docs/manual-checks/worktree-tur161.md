# Manual checks: worktree-tur161 (TUR-161, one stop path; notes after an interrupted recording)

`src-tauri/src/recording/stop.rs` (new, moved out of `recording.rs`): the
user's Stop, a failed tick (`fail_mid_recording`) and a ticker that never
spawned all end through `stop::close`: stop the session, finish the
transcript, then end. `Ending::after_stop` is the one place that decides the
after-Stop handling (`agent_run::finish_then_run`: the `Transcribing` busy
marker for retention, and the notes run when `agent.auto_run` is on). A
Stop and an interrupted recording get it; a start that never happened does
not (its folder is removed).

`src-tauri/src/live_transcript/`: every failure sentence now ends "its audio
is kept in the meeting folder" instead of promising a transcription from the
saved audio, which no command does. `open()`, `start_session` and the
clean-up `finish()` run inside `guarded()`, so a panic there fails
transcription at once. The first failure while the meeting records posts one
desktop notification (`notify::transcription_failed`).

Headless tests cover: the close order for all three endings; an interrupted
recording starts the notes run with `auto_run` on and not with it off, the
same as a Stop; a start that never happened never does; a panicking session
stop still finishes the transcript and ends; a panicking engine load and a
panicking `start_session` (with a panicking clean-up `finish`) fail at once;
only the first failure while recording notifies, and none after Stop.

## Run by hand

1. **An interrupted recording gets its notes.** Test meetings folder (never
   your real one) on a small disk image that fills up mid-meeting (for
   example a 50 MB APFS image made with Disk Utility; verify it fills during
   a recording), `agent.auto_run: true`, an agent set up. Record until the
   disk is full.
   Expect: the "meet-ai stopped recording" notification as before, then the
   meeting view shows *Writing notes…* and the notes of what was recorded.
   With `agent.auto_run: false`, no run starts and *Make notes now* is offered.
   Why skipped: needs a mic, a signed build, the running app and a signed-in
   agent CLI.
2. **A failed device reopen does the same.** Record on a USB microphone and
   unplug it mid-meeting with no other input (verify that the reopen fails on
   this Mac rather than falling back to the built-in mic).
   Expect: as in 1.
   Why skipped: needs real hardware and a signed build.
3. **A hidden window hears a transcription failure.** Start a recording with
   ⌘⇧R with the window hidden, on the whisper engine with no model installed
   (or the Apple engine with its locale missing).
   Expect: one "meet-ai stopped transcribing" notification within seconds,
   whose text ends "Recording continues, and its audio is kept in the meeting
   folder." No second one for the same meeting. The recording carries on.
   Why skipped: needs the running app and notification permission.

## Notes

- No "Transcribe again from audio" command was added (ticket default). A
  follow-up could back one with the after-Stop batch path
  (`live_transcript::after_stop`, D8b / TUR-137).
