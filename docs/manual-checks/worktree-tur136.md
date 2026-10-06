# Manual checks: tur136

TUR-136: Record starts at once; the system-audio permission check (the chime)
runs during the recording, on a second copy of the recording's own system
stream. A denied system audio drops the system track and the recording goes
on microphone-only. Headless tests cover the verdict and drop logic (fake tap
and fake clock), the tee fan-out and hush, and a 10 s fake check that does not
hold the start. Nothing below was run: each needs a signed build and real
audio hardware.

## Run by hand (Wave H, signed build)

1. With System Audio Recording and Microphone both on for meet-ai, press
   Record (button, ⌘⇧R and the menu bar each once).
   Expect: the window shows Recording in under 1 s. In the log, the time from
   the `permission check before recording` line to `starting microphone
   capture` and then to the ticker's first checkpoint is well under 1 s
   (it was about 10 s before). About 1.2 s after the start you hear the chime
   once, then a `system-audio check during recording` line with
   `state=Granted`.
   Why skipped: needs the signed app, a microphone and speakers.
2. Talk over the chime in that recording, stop it, open `transcript.md`.
   Expect: no line for the chime (no "beep", "[music]", "ding" or similar).
   A second or so of your words around the chime may be missing from the
   live transcript (it is hushed for about 1 s per play); `mic.wav` still has
   them and the chime.
   Why skipped: needs a real engine, a real microphone and speakers.
3. Turn System Audio Recording off for meet-ai in System Settings, Privacy &
   Security. Play something (a video) and press Record.
   Expect: the recording starts at once anyway. You hear the chime twice
   (about 1.2 s and 2 s in). Within about 5 s the banner reads "System audio
   is off: System Audio Recording is turned off for meet-ai in System
   Settings, so this recording goes on with your microphone only." The
   recording keeps running; Stop still works. `audio/segments.json` has a
   second segment with `"reason": "system_audio_denied"` and `"sys_rate": 0`;
   `mic.wav` covers the whole recording. Swapping the output device (AirPods
   in) later in the same recording does not bring the system track back.
   Why skipped: needs a real TCC denial on a signed build (verify it: the
   tap's all-zeros denial is FINDINGS §10.1, not re-measured here).
4. Turn System Audio Recording back on, record again.
   Expect: the banner goes away once the check finds the chime (the window
   now gets the check's answer during the recording, not before it).
5. Turn Microphone off for meet-ai and press Record.
   Expect: refused at once, as before, with the "Microphone is turned off"
   message on the idle status; no folder is left behind.
6. Windows and Linux: press Record.
   Expect: the recording starts at once and the start sound plays once
   during the recording; it does not show up in `transcript.md`. Verify it.
   Why skipped: needs a real Windows or Linux machine.

## Not covered

- A batch re-transcription of the WAVs (`cargo run -p stt --example
  offline_meeting`) hears the chime in both tracks; nothing in the app runs
  one. Verify what an engine writes for it if that ever ships.
