# Manual checks: TUR-145 backup stops (silence, sleep)

Ten minutes with no speech asks to stop with the countdown card; the computer
going to sleep stops and saves at once (SPEC A32). The decision logic is
unit-tested headless: the silence timer and its loop with fake audio levels,
a fake clock and a fake card (`recording/backup_stop/{silence,listen,speech}.rs`),
the bounded wait the Windows and Linux hooks use, the setting, and the meeting
page's wording. The Windows and Linux hook files were compiled (and clippy'd)
in a throwaway crate here, for `x86_64-pc-windows-msvc` and on the macOS host
for the zbus code; CI's `rust (windows | linux)` builds them for real. Nothing
below was run: each needs the signed app, real audio, or a real sleep.

## Run by hand

1. macOS, signed build: start a recording, talk for a minute, close the lid
   for 30 s, open it.
   Expect: the recording stopped as the lid closed (log: "the Mac is going to
   sleep", then "recording stopped" with `reason=sleep`); the meeting plays to
   the moment of the lid close in QuickTime; its page's meta line ends with
   "Stopped when this Mac went to sleep", not "Interrupted".
   Why skipped: needs the signed app, a real mic and a real sleep.
2. macOS, same with Apple menu → Sleep instead of the lid.
   Expect: as 1.
   Why skipped: as 1.
3. Windows: start a recording, Start → Power → Sleep, wake after 30 s.
   Expect: as 1, with "this PC" and the log line "the PC is going to sleep".
   On a Modern Standby (S0ix) laptop, check whether `PBT_APMSUSPEND` arrives
   at all when the lid closes: verify it, and note the answer here.
   Why skipped: needs a real Windows machine.
4. Linux (GNOME or KDE, systemd): start a recording, suspend from the menu.
   Expect: as 1 ("this computer"). While meet-ai runs, `systemd-inhibit
   --list` shows a `delay` lock for `sleep` by meet-ai, and it is back after
   the wake. Verify the distribution's `InhibitDelayMaxSec` (logind.conf) is
   at least 4 s.
   Why skipped: needs a real Linux machine with logind.
5. Silence: start a recording with the call muted and say nothing for 10 real
   minutes.
   Expect: the countdown card, "No one has spoken for 10 minutes", with a
   10 s ring, Stop now and Keep recording; at zero the recording stops and its
   page says "Stopped after 10 minutes of silence". Check the line is not
   cut off in the 220 px card (its tooltip and VoiceOver have the full line
   either way).
   Why skipped: needs the running app and 10 real minutes.
6. Silence, Keep recording: as 5, press Keep recording.
   Expect: the card goes, the recording goes on, and the card is back 10
   minutes later if it stays silent.
   Why skipped: as 5.
7. Silence, someone speaks during the countdown.
   Expect: the card goes away on its own and the recording goes on.
   Why skipped: as 5.
8. Silence with `transcription.live: false`, and with the Apple engine and
   whisper.
   Expect: the same card after 10 silent minutes in every case (the stop has
   its own copy of the audio).
   Why skipped: as 5.
9. Settings → Notifications → "Stop after 10 min of silence" off, then 10
   silent minutes.
   Expect: no card; `config.jsonc` has `"stop_after_silence": false` under
   `detection`. Turning it on mid-recording starts the 10 minutes from then.
   Why skipped: needs the running app.
10. A meeting with music or a video playing and nobody speaking.
    Expect: earshot does not count music as speech in most cases, so the card
    may appear; note how often here.
    Why skipped: needs real audio.

## Known

- The stop reason is kept in memory for as long as the app runs (owner's
  call, `.agent` A2): after a restart such a meeting reads as finished, which
  it is.
- The meeting list rows still show the line count for these meetings; only
  the meeting's own page names the backup stop.
