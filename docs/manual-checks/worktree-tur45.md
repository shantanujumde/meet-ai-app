# Manual checks: worktree-tur45 (TUR-45, audio retention job)

`crates/store/src/retention.rs`: the planner (`survey`, `plan`) and the delete
step (`apply`). `src-tauri/src/retention.rs`: the schedule (about 30 s after
launch, then every 24 h, and right after a transcript is final when
`retention_days` is `0`) and what counts as busy.
`src-tauri/src/config/audio_section.rs`: reading `audio.retention_days`.
`src/ui/AudioRetentionRow.tsx`: the line in Settings → Files.

Headless tests cover the planner on fixture meetings (older than N deleted,
newer kept, exactly N kept, `0`, `-1`, busy skipped, missing or empty
transcript skipped, dating by `meeting.md`, folder name or WAV mtime, locked
file skipped and retried on Unix), the busy wiring and the config reader.

## Run by hand

1. **The launch pass deletes old audio and logs it.** With a test meetings
   folder (never your real one), add a meeting folder dated more than 7 days
   ago with a non-empty `transcript.md` and `audio/mic.wav`,
   `audio/system.wav`, `audio/segments.json`. Launch the signed app and wait
   40 s.
   Expect: both WAVs are gone; `segments.json`, `audio/`, `transcript.md`,
   `notes.md`, `meeting.md` and `tickets/` are still there. `.app/logs/meet-ai.log`
   has one `deleted old meeting audio` line for that meeting (files and bytes)
   and one `audio retention pass done` line. The meeting still opens, and is
   not labelled Interrupted.
   Why skipped: needs the running app.
2. **`0` deletes right after the transcript.** Set
   `"audio": { "retention_days": 0 }` in `config.jsonc`, record a short
   meeting and stop.
   Expect: the WAVs stay while recording, while the transcript finishes and
   while *Writing notes…* shows; they are gone a moment after the notes run
   ends (or right after Stop's transcript is final when `agent.auto_run` is
   off). The log says so.
   Why skipped: needs a mic, a signed build and (for the notes step) a
   signed-in agent.
3. **`-1` keeps everything.** Set `retention_days` to `-1` and relaunch with
   an old meeting in the folder.
   Expect: nothing deleted; Settings reads "Audio is kept forever".
   Why skipped: needs the running app.
4. **Settings line.** Open Settings → Files with `retention_days` unset, `0`,
   `-1` and `30`.
   Expect: "Audio is kept for 7 days.", "Audio is deleted once the transcript
   is done.", "Audio is kept forever.", "Audio is kept for 30 days.", each
   followed by "Transcripts and notes are always kept." A bad value (`-5`,
   `"7"`) shows 7 days and logs a config warning. Covered headless by
   `src/ui/AudioRetentionRow.test.tsx`.
   Why skipped: opens a window.
5. **Windows: a locked WAV is retried.** On Windows, open an old meeting's
   `mic.wav` in a player that holds it open, then let the pass run.
   Expect: `audio file in use; will retry next run` in the log, the other WAV
   deleted, and the locked one deleted on the next pass once the player
   closes. The `#[cfg(windows)]` test
   `a_locked_wav_is_skipped_and_deleted_on_the_next_run` covers this once a
   Windows test job runs `cargo test -p store`.
   Why skipped: no Windows machine here, and `store` is outside
   `just check-windows` (rusqlite compiles C for the target), so not even a
   cross-`cargo check` of it runs on this Mac.
6. **Linux.** `cargo test -p store retention` on Linux.
   Expect: all pass, including the Unix locked-file test (it returns early,
   passing, when run as root).
   Why skipped: no Linux machine here; no Linux test job on main yet.
