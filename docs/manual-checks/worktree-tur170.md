# Manual checks: tur170

TUR-170: stale or racing state in Settings and the meeting view. Each fix
has a Vitest or Rust unit test; these need the running app, a signed build,
real audio or a real agent CLI, so they were not run here.

## Run by hand

1. Settings: in Agent pick None, then scroll to Tracker without leaving the page.
   Expect: Tracker says "No agent is set up". Pick Codex: Tracker says
   "Tickets are sent by Codex", the tip names `codex mcp add`, and the server
   list is asked again (the "Checking your agent's servers…" line shows).
   Changing only the model does not ask again.
   Why skipped: needs the running app and the agent CLIs.
2. Settings, speech: download a Whisper model, then pick Whisper and at once a
   different model.
   Expect: the radios end on the last pick, and config.jsonc says the same.
   Why skipped: needs the running app and a model file.
3. Add a user hook that exits 1 on `on_meeting_end`. Record a short meeting,
   press Stop, and stay on the meetings list until the hook has run. Then open
   the meeting, leave it, and open it again.
   Expect: "Hook failed: on_meeting_end (…)" both times. After a restart the
   note is gone (only the log keeps it).
   Why skipped: needs a signed build recording real audio.
4. With no meetings yet, press ⌘⇧R and look at the empty list's
   "Start recording" button while it says Starting.
   Expect: disabled until the recording is live; clicking it does nothing.
   Why skipped: needs a signed build and the microphone.
5. Settings, Files, Change: if the confirm dialog cannot open (dialog plugin
   missing from the capability), the Files card shows the error instead of
   nothing. Hard to provoke on a normal build; verify it in a dev build whose
   capability does not allow the dialog's ask (check the permission name in
   the dialog plugin's docs first).
   Why skipped: needs a build with a changed capability.
6. Settings, Notes after the call: set "Only when I ask". Open a meeting.
   Expect: the notes switch says "On: your agent writes notes from the
   transcript when you click Make notes now."
   Why skipped: needs the running app.
7. Start a recording and reload the window (or open it from the menu bar)
   during Starting.
   Expect: the titlebar ends on Recording with the timer, never stuck on
   "Starting…".
   Why skipped: needs a signed build and the microphone; the ordering is
   unit-tested with fake events.
8. Record, then start a meetings-folder move (Settings, Files, Change) and,
   while the dialog flow checks, press ⌘⇧R.
   Expect: the recording stops; no "moving your meetings folder" notice. The
   move is then refused only because a recording was going, or it runs once
   idle. With nothing recording, ⌘⇧R during a move is still refused.
   Why skipped: needs a signed build, and the window between the two is short.
9. Record on speakers (banner "No headphones" shows), then set
   `audio.warn_no_headphones` to false in config.jsonc.
   Expect: the banner goes within about 3 s. Set it back to true: it returns
   within about 3 s.
   Why skipped: needs a signed build, speakers and a recording.
10. Put `"app": { "show_in_dock_when_closed": "yes" }` in config.jsonc and open
    Settings.
    Expect: the Menu bar section shows "This setting in config.jsonc was not
    valid…"; flipping "Show in Dock when the window is closed" clears it.
    Why skipped: needs the running app.
11. Let another app own ⌘⇧R, then run setup again (Settings, Show setup again)
    to the folder step.
    Expect: it says to use the Record button and that ⌘⇧R is unavailable,
    not "Press ⌘⇧R from anywhere".
    Why skipped: needs the running app and another app holding the shortcut.
