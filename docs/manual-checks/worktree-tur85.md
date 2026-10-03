# Manual checks: worktree-tur85 (TUR-85, retention never deletes audio it cannot prove is safe)

`crates/store/src/retention.rs`: a meeting's WAVs are planned for deletion
only when its transcript has text, `audio/.incomplete` is absent, and its
recording ended cleanly (the meeting list's Interrupted classifier, passed in
by the app). `crates/store/src/folder_name.rs`: `create_meeting_folder`
writes `audio/.incomplete`. `src-tauri/src/agent_run.rs` →
`retention::transcript_finished` removes it only when live transcription
ended `stopped` (no failure, no timeout).
`src-tauri/src/config/audio_section.rs`: `retention_policy` pauses the job
when `config.jsonc` cannot be read or parsed or `audio` is invalid; the 7-day
default applies only when the file or key is absent.
`src/ui/AudioRetentionRow.tsx`: "Audio cleanup paused: config.jsonc could not
be read (…)" in Settings → Files.

Headless tests cover: interrupted meeting kept, failed live transcription
kept, crash-then-relaunch kept (marker on disk, idle app), a clean meeting
deleted after N days and not before, an unreadable config (a folder in the
file's place), a config that does not parse and an invalid `audio` section
delete nothing, an absent file or key is 7 days, and the Settings line.

## Run by hand

1. **A crashed recording keeps its audio after relaunch.** With a test
   meetings folder (never your real one) and `"audio": { "retention_days": 0 }`,
   start a recording, talk until lines show, then `kill -9` the app. Relaunch
   and wait 40 s.
   Expect: `audio/.incomplete` and both WAVs are still there; the meeting is
   labelled Interrupted; the log's `audio retention pass done` line has
   `deleted=0`.
   Why skipped: needs a mic, a signed build and the running app.
2. **A failed live transcription keeps its audio.** Same config. Record a
   meeting, make live transcription fail mid-meeting (for example remove the
   speech model or make `meet-stt` unavailable before Start so the engine
   cannot open), then press Stop.
   Expect: the window says transcription stopped; the log has `the transcript
   is not complete; keeping this meeting's audio`; the WAVs and
   `audio/.incomplete` stay.
   Why skipped: needs a mic, a signed build and the running app.
3. **A clean meeting still loses its audio.** Same config, record a short
   meeting normally and press Stop.
   Expect: `audio/.incomplete` is gone once the transcript is final, and the
   WAVs are deleted as before (after the notes run when `agent.auto_run` is on).
   Why skipped: needs a mic and a signed build.
4. **A broken config pauses cleanup.** With an old, cleanly recorded meeting
   in the test folder, put a syntax error anywhere in `config.jsonc` (or set
   `"retention_days": -5`), relaunch and wait 40 s, then open Settings → Files.
   Expect: no WAV deleted; the log has `audio cleanup paused: config.jsonc
   could not be read` with the reason; Settings shows "Audio cleanup paused:
   config.jsonc could not be read (…). No audio is deleted until it is
   fixed." Fix the file, reopen Settings: the normal line is back, and the
   next pass deletes as usual.
   Why skipped: needs the running app and a window.
5. **Meetings recorded before this change.** Launch with an old meeting
   folder made by v0.3.x (no `audio/.incomplete`).
   Expect: deleted after N days only if the list shows it as finished; an
   Interrupted one keeps its WAVs.
   Why skipped: needs the running app.

## Notes

- A meeting whose transcription never reaches a clean `stopped` (the
  recording failed mid-way, the ticker could not start) keeps
  `audio/.incomplete`, so its audio is kept until the user deletes it.
  That is the safe side; a later re-transcribe feature should remove the
  marker when it writes a whole transcript.
- `"retention_days": null` is read as "key absent" (7 days), as before.
