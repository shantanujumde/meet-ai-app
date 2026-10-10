# Manual checks: worktree-tur148 (TUR-148, a transcript with gaps keeps its audio)

`src-tauri/src/live_transcript/supervise.rs`: each feeding thread adds its
tee's `dropped_frames()` to the board (`Scope::dropped` in `board.rs`) when
it ends. The count rides on `live_transcript::Status::dropped_frames`, which
is `#[serde(skip)]`: the window's payload and `bindings.ts` are unchanged, and
the status still ends `stopped`, so nothing new is shown.
`src-tauri/src/retention.rs` → `transcript_finished` leaves
`audio/.incomplete` in place (the TUR-85 marker, so retention keeps the WAVs)
when that count is above zero, and logs `the speech engine fell behind, so
the transcript has gaps; keeping this meeting's audio` with `dropped_frames`.

Headless tests cover: a full tee queue (frames dropped) stopped cleanly
leaves the marker; a queue that never filled removes it; the next meeting
starts at zero; a `stopped` status with drops keeps the WAVs through a
`retention_days: 0` pass; the status JSON has no new field.

## Run by hand

1. **An engine that falls behind keeps the audio.** With a test meetings
   folder (never your real one), `"audio": { "retention_days": 0 }` and the
   whisper engine with a large model on a slow machine (or any setup where
   the engine cannot keep up), record a meeting long enough that the log
   shows `the speech engine fell behind; those frames reached the WAV but
   not the live transcript`, then press Stop.
   Expect: the window ends as before (no new message); the log has `the
   speech engine fell behind, so the transcript has gaps; keeping this
   meeting's audio` with a `dropped_frames` count; `audio/.incomplete` and
   both WAVs stay after the notes run and the next retention pass.
   Why skipped: needs a mic, a signed build, the running app and an engine
   slow enough to fall behind about 20 s of audio (verify it on the machine).
2. **A normal meeting still loses its audio.** Same config, a short meeting
   on the Apple engine, press Stop.
   Expect: `audio/.incomplete` is gone once the transcript is final, and the
   WAVs are deleted as before.
   Why skipped: needs a mic and a signed build.

## Notes

- There is no "partial" state in the UI; per the ticket's defaults nothing
  new is shown. A meeting kept this way can be re-transcribed from its WAVs.
