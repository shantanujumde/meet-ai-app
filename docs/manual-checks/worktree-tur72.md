# Manual checks: TUR-72 (permission check says "not allowed" on a granted Mac)

Ticket: https://linear.app/meet-ai-app/issue/TUR-72/permission-check-says-not-allowed-on-a-granted-mac-it-stops-listening

None of this could run here: it needs the system-audio tap and its macOS
permission, real speakers, a signed build and the running app. The headless
tests in `crates/audio/src/permission_check/verdict/tests.rs` drive the real
listening loop (`chime::listen_live`) and the real verdict with a fake tee feed
on a fake clock: a first frame up to 4 s late, 500 ms chunks arriving slower
than real time, a tap that stalls right after the chime (the owner's log), a
tap that closes, a tap that never delivers, and denied taps that start late.

## What was wrong

`check_system` set its wall-clock deadline (`worst_case_millis() + 1500` =
4.34 s) right after the tap started, but `play_until_heard` keeps time in
captured samples. The log's `captured=22826` is 1.43 s of audio in 4.34 s of
wall time, so ~2.9 s went by before audio arrived (or it arrived far slower
than real time). The chime played at 19 200 samples, the deadline hit 0.23 s
later, and an unfinished listen was reported as `Denied`.

Which of the two (late first frame or slow delivery) happens on the owner's
Mac is not known yet; the new log fields below tell them apart.

## What changed

- The wall-clock safety net now starts at the first frame: up to 5 s
  (`FIRST_FRAME_TIMEOUT_MILLIS`) for the first frame, then
  `worst_case_millis()` + 2 s (`DEADLINE_HEADROOM_MILLIS`) from it.
- `Denied` only when both plays were listened to for their full window and
  the chime never came back. A cut-short listen is `Unmeasurable` (the app
  shows "could not finish checking" and lets recording start, as before for
  `Unmeasurable`), with a detail saying it was cut short and how much audio
  arrived.
- The log line `system-audio permission check finished` adds `ended`
  (`heard|window|deadline|tap_closed`), `first_frame_ms`, `peak` and `rms` (of
  the audio captured after the first play).

## Checks to run

| # | What to do | Expected | Why skipped here |
|---|---|---|---|
| 1 | Owner's Mac, signed build of this branch, Microphone and System Audio Recording Only both on for meet-ai. Onboarding or Settings: press "Check again". | Granted. One chime. Log line shows `present=true ended=heard`, plus `first_frame_ms=Some(..)`, `peak`, `rms`. Note the `first_frame_ms` value in the ticket. | Needs a signed build, the tap permission and the running app. |
| 2 | Same Mac, press Record. | One chime, recording starts, log `permission check before recording state=Granted`. | Same. |
| 3 | If check 1 is not Granted: read the log line. | `ended=deadline` with `first_frame_ms` near 5000 or `None`: the tap starts too late or stalls; raise `FIRST_FRAME_TIMEOUT_MILLIS` or look at the tap start. `ended=window` with `peak=0 rms=0`: a real denial (exact zeros). `ended=window` with real `peak`/`rms`: audio arrives but not our chime (muted output, other device). | Needs the real machine. |
| 4 | Turn System Audio Recording off for meet-ai, press "Check again". Turn it back on afterwards. | Two chimes, then Denied; log `ended=window peak=0 rms=0`. | Needs changing real macOS permissions. |

## Choices made without an answer

- A listen whose first play was heard out in full but which ended before the
  retry could play is also `Unmeasurable`, not `Denied`: the retry exists to
  avoid a false denial, so a check without it has not "listened fully, at
  most `MAX_PLAYS` times".
- `FIRST_FRAME_TIMEOUT_MILLIS = 5000`, `DEADLINE_HEADROOM_MILLIS = 2000`. A
  tap that never delivers now costs 5 s instead of ~4.3 s before recording
  starts; a stalled tap at most ~9.8 s.
- The verdict and log moved to `crates/audio/src/permission_check/verdict.rs`
  (platform-free, `pub` so TUR-42's platform module can call it without new
  `cfg` lines), keeping the `permission_check.rs` diff inside `check_system`.
