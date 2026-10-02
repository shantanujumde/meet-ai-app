# Manual checks: c-tur10

TUR-10: the notes run starts on its own when a call ends (SPEC A11). After
Stop, once `transcript.md` is final, `src-tauri/src/agent_run.rs` runs the
user's agent CLI in the background, if `agent.auto_run` is on and the meeting
is not marked `agent_notes: off`. The meeting view shows *Writing notes…* with
Cancel, then the notes, or the reason it failed with Retry. One run per
meeting at a time. A notification says the notes are ready when the window is
not in front. Quitting kills a running run and leaves the meeting as it was.

Tested headless with the fake harness (`agent::fake::FakeHarness`): success,
each failure, cancel, one run per meeting, and quit mid-run followed by a
Retry on a fresh state (the next launch). The window side is tested with
vitest against a mocked IPC.

## Run by hand

1. **Auto run after a real call.** Sign in to Claude Code, leave `agent` in
   `~/Meetings/.app/config.jsonc` at its defaults, record a short meeting
   with a few spoken tasks in it (`just bundle-signed`, then open the app),
   press Stop.
   Expect: the meeting view shows *Writing notes…* with Cancel within a
   second of the transcript finishing; recording a second meeting at once is
   not held up; after the run, the Summary, Decisions, Action Items and Open
   Questions show and `tickets/TICK-NNNN.md` files exist.
   Why skipped: needs the mic, system audio, the signed bundle and a
   signed-in CLI.
   Also check from a Finder launch with an npm-installed `claude`: the run
   gets the CLI's own folder first on `PATH` (TUR-6's `search_path_with`),
   so `#!/usr/bin/env node` finds `node`.
2. **Notification.** Same as 1, but switch to another app before the run
   ends.
   Expect: a macOS notification that the notes are ready. With the meet-ai
   window in front, no notification.
   Why skipped: needs the running app and macOS notification permission.
3. **Cancel.** Same as 1, press Cancel while it says *Writing notes…*.
   Expect: a "cancelled" failure saying nothing was written, with Retry; no `claude` process is left
   running (`pgrep -fl claude`); `meeting.md` unchanged.
   Why skipped: needs the running app and a real CLI.
4. **Quit mid-run.** Same as 1, press ⌘Q while it says *Writing notes…*,
   then reopen the app and open the meeting.
   Expect: no notes, no error, a button to write notes; pressing it runs
   and succeeds. No orphaned `claude` process after the quit.
   Why skipped: needs the running app.
5. **Not signed in.** Sign out of Claude Code (`claude`, then `/logout`),
   record and stop.
   Expect: the failure says Claude Code is not signed in and shows the
   command to run in Terminal. Sign back in, press Retry: notes appear.
   Why skipped: signing out changes the user's machine; needs a real CLI.
6. **auto_run off.** Set `"auto_run": false` in the `agent` section, record
   and stop.
   Expect: no run starts; the meeting view offers to write notes by hand.
   Why skipped: needs the running app.
7. **Codex picked.** Install and sign in to Codex, set `"harness": "codex"`
   and `"model"` to a Codex model, record and stop.
   Expect: notes appear as with Claude Code; `meeting.md` says
   `analyzed_by: codex`.
   Why skipped: needs the running app and a signed-in Codex.

## Decisions taken without an answer

- Finding the CLI: only through `Harness::detect()`, per Shann (no PATH or
  login-shell lookup in `agent_run`). TUR-6 (#45) merged before this
  branch's rebase, so `detect()` is real.
- The auto run starts only after a user Stop (button, ⌘⇧R, menu bar). A
  recording that stops on its own because of an error (`fail_mid_recording`)
  does not start one; Retry / "Write notes" in the meeting view does.
- Nothing about a run is saved to disk. After a quit or a crash the meeting
  shows "No notes yet" and a Write notes button, which is the Retry.
