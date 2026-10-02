# Manual checks: TUR-14 (start chime plays once)

Ticket: https://linear.app/meet-ai-app/issue/TUR-14/recording-start-plays-the-chime-11-times-play-it-once

None of this could run here: it needs real speakers, the system-audio tap and
its macOS permission, and the running app. The headless tests in
`crates/audio/src/chime/attempts/tests.rs` cover the timing logic with a fake
tap (normal start, the 1.07 s settle, a late settle, a denied tap, slow
Bluetooth output, a tap that stops early).

## What changed, in short

The permission check no longer loops the chime across ~3.2 s. It starts the
system-audio tap, waits until the tap has delivered 1.2 s of audio (past the
measured 1.07 s settle time), plays the chime **once**, and checks the captured
audio live. As soon as the chime is heard, the check ends. If it is not heard
within the chime plus 600 ms, it plays **one** more time, no earlier than 2 s
into the capture, then reports. Never more than 2 chimes.

So on a normal start you hear one chime about 1.2 s after pressing Record (the
old loop started at once but never stopped early). A denied Mac now hears 2
chimes and gets its answer after about 2.8 s, not 3.5 s.

## Checks to run

| # | What to do | Expected | Why skipped here |
|---|---|---|---|
| 1 | Signed build, built-in speakers, volume normal. Press Record 5 times in a row (stop each one after a few seconds). | One chime per Record press, every time. Recording starts; no "denied" screen. | Needs speakers, the tap's macOS permission and the running app. |
| 2 | Same as 1 with AirPods connected as the output. | One chime per press, heard in the AirPods. No "denied" screen. Bluetooth adds latency; 600 ms of listening per play should cover it. If you ever hear 2 chimes here, note it: the listen window or the 1.2 s settle wait may need to grow. | Needs AirPods and the app. |
| 3 | Onboarding / Settings permission check ("check again"). | One chime, result reads granted. | Needs the app window and real permission state. |
| 4 | Hardware test, granted Mac: `cargo test -p audio --test permission_check check_system -- --ignored --nocapture`. | Passes; the log line shows `plays=1`. | `--ignored` real-hardware tests are not allowed in this run. |
| 5 | Turn off system-audio recording for meet-ai in System Settings, press Record. | Exactly 2 chimes, then the permission screen. Turn it back on afterwards. | Needs changing real macOS permissions. |

## Found along the way

- The chime detector (`chime::heard`) can be fooled by loud white noise: a
  test with noise peaking at 0.41 (what TUR-4 saw in the tap's unsettled first
  second) read as "chime heard". The new check only listens to audio captured
  *after* it plays the chime, at 1.2 s, so the unsettled stretch is not
  searched. The old looped check did search it. A denied tap sends exact
  zeros, so this can never turn a denial into a pass. It could still matter
  if a real tap settles after 1.2 s **and** its unsettled audio is broadband
  noise. Worth a follow-up ticket to measure what the unsettled audio really
  looks like.
- If the tap stops delivering audio before the chime plays, the check now says
  "could not measure" instead of "denied".

## Choices made without an answer

- Approach 1 from the ticket (wait for the tap to settle, chime once, one
  retry only if unheard, cap 2), as the run's defaults said.
- Settle wait 1.2 s (`chime::SETTLE_MILLIS`): the 1.07 s measured in TUR-4 plus
  a small margin, so the cue is not delayed longer than needed. If check 1 or
  2 shows a second chime now and then, raise it toward 1.5 s.
- `ONSET_TIMEOUT_MILLIS` stays 2000 ms; it now sets the earliest time of the
  retry instead of how long the chime loops.
- The `#[ignore]`d closed-loop hardware tests (`tests/system_closed_loop.rs`,
  `tests/mic_closed_loop.rs`) still use the looped chime. They are test tools,
  not the app, so they were left as they are.
