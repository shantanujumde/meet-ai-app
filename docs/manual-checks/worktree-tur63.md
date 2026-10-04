# Manual checks: tur63

TUR-63: user hooks. `hooks.on_transcript_ready`, `hooks.on_analysis_complete`
and `hooks.on_meeting_end` in `config.jsonc` run the user's command through
the platform shell (`sh -c`; `cmd /D /S /C` on Windows; PowerShell `-File`
for a `.ps1`), with the meeting folder in `MEETAI_MEETING_DIR` and as the last
argument. `hooks.timeout_secs` (default 30) stops the whole process tree via
`agent::ProcessTree`. Output (64 KiB each of stdout and stderr) goes to the
app log. A failure is logged and shown as "Hook failed: <name> (...)" in the
meeting view; it never blocks the meeting.

Headless tests in `src-tauri/src/hooks/tests.rs` cover env + argument, the
timeout killing a child the hook started, a failing hook being reported with
the meeting untouched, the output cap, `~`/`$HOME` expansion, and when each
moment fires. On macOS they ran here; Windows and Linux run in CI.

## Run by hand

1. In `~/Meetings/.app/config.jsonc` set
   `"hooks": { "on_meeting_end": "echo \"$MEETAI_MEETING_DIR\" >> ~/hook.log",
   "on_transcript_ready": "echo ready >> ~/hook.log",
   "on_analysis_complete": "echo notes >> ~/hook.log" }`, record a short
   meeting with a signed build and an agent set up, then stop.
   Expect: `~/hook.log` gets the meeting folder (twice: env and argument), then
   `ready`, then `notes` once the notes are written.
   Why skipped: needs a signed build, mic permission and a signed-in agent.
2. Set `"on_meeting_end": "exit 3"`, record and stop.
   Expect: the meeting finishes normally (transcript, notes) and the meeting
   view shows "Hook failed: on_meeting_end (exited with status 3)"; the app
   log has a `hook exited with status 3` warning.
   Why skipped: needs the running app. Note: the note shows only while that
   meeting's view is open when the hook fails (it is not stored).
3. Set `"on_meeting_end": "sleep 100 & sleep 100"`, `"timeout_secs": 2`.
   Expect: after about 2 s the log says the hook ran too long, and
   `ps -ax | grep 'sleep 100'` shows nothing left.
   Why skipped: needs the running app (the same case is tested headless).
4. Windows: `"on_meeting_end": "C:\\hooks\\end.ps1"` and a `.bat`.
   Expect: both run, `$args[0]` / `%1` is the meeting folder.
   Why skipped: no Windows machine here; CI runs the cmd tests only. Measure
   whether PowerShell's execution policy on a managed machine still blocks
   `-ExecutionPolicy Bypass`.

## Notes

- `on_meeting_end` fires when the recorder goes from a meeting back to idle;
  `on_analysis_complete` when a notes run reaches `done`. Both follow the
  events the window already gets, so `recording.rs` and `agent_run.rs` (owned
  by other tickets) are unchanged. `on_transcript_ready` fires from
  `retention::Transcribing::settled` when `transcript.md` is final.
