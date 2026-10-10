# Manual checks: TUR-160

Quitting now waits, bounded, for a recording that is Starting or Stopping to
settle, stops it, and stops every Sync run and agent Test run (their CLI
process trees) before the app ends. "Stop and quit" does this on a quit
thread and quits once it is done; logout, shutdown and an updater restart do
it in `RunEvent::Exit` on the main thread. The decision logic and the waits
are unit-tested headless (`recording::settle` with a fake recorder,
`cancels`, `sync::tests::shutdown_*`, `agent_setup::tests::shutdown_*`,
`lifecycle::tests`). Nothing below was run here: each needs the signed app,
a microphone, an OS logout or a signed-in agent CLI.

## Run by hand

1. Press ⌘⇧R, then ⌘Q at once while the window or overlay still shows
   Starting. Click Stop and quit.
   Expect: the button reads "Stopping…", the app quits a moment later, and on
   the next launch that meeting is a normal (not Interrupted) meeting with
   audio. The log has `recorder finished for quit` with `phase=Idle` and
   `finished the work left at quit`.
   Why skipped: needs the signed app and a microphone.
2. Record for 30 s of speech, press Stop, and press ⌘Q while it says
   Stopping; confirm Stop and quit if asked.
   Expect: the app quits after the stop finishes (up to about 15 s), and the
   transcript has its last lines, in time order.
   Why skipped: needs the signed app and a microphone.
3. While recording, log out of macOS (or `shutdown -r` on a test machine).
   Expect: the meeting is complete on next login, and the log has
   `finished the work left at exit` with how long it took.
   Why skipped: needs an OS logout.
4. With a tracker set up, press Sync on a task and quit (menu bar Quit)
   while it shows Sending.
   Expect: no `claude`/`codex` process left (`pgrep -fl claude`), no issue
   created in the tracker for that press, and the next Sync makes exactly
   one issue.
   Why skipped: needs a signed-in agent CLI and a tracker.
5. Settings, Agent, press Test and quit while it runs.
   Expect: no agent CLI process left after the app is gone.
   Why skipped: needs a signed-in agent CLI.

## Known

- The tracker check ("Send test ticket", `sync/check.rs`) is not covered:
  it creates nothing, but its CLI can still outlive a quit. Not in this
  ticket's scope.
