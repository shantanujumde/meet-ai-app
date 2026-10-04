# Manual checks: tur60

TUR-60: meeting detection on Windows and Linux. `crates/detect/processes.json`
now has an `os` field per entry and Windows and Linux names; only this OS's
entries are matched. Apps are grouped by label, so `ms-teams.exe` plus
`ms-teams_modulehost.exe` is one Teams session. "Mic and speakers in use" is
read from WASAPI audio sessions on Windows and from the PulseAudio/PipeWire
stream lists on Linux, without meet-ai's own pid.

Unit tests cover the per-OS name lists, the Teams grouping, own-pid filtering
and the libpulse property parsing. They run in the `rust (macos-26)`,
`rust (windows)` and `rust (linux)` CI jobs. The Linux code was written on a
Mac and only compiled in CI. No test talks to a real sound server or device.

## Run by hand

1. Windows 11, a build from this branch, detection on. Start Zoom and join a
   test call (zoom.us/test) with mic and speakers on.
   Expect: one "Zoom is open." prompt when Zoom starts, and no second
   "microphone and speakers" prompt after 20 s of the call.
   Why skipped: needs a real Windows machine, Zoom and an audio device.
2. Windows, new Teams: start Teams and a test call (Settings → Devices →
   Make a test call).
   Expect: one "Microsoft Teams is open." prompt in total, even though
   `ms-teams.exe` and `ms-teams_modulehost.exe` both run.
   Why skipped: same as 1, plus a Teams account.
3. Windows, a call in a browser (Google Meet in Edge), no meeting app open.
   Expect: after 20 s of the call, one "Your microphone and speakers are both
   in use" prompt. Then start recording in meet-ai, stop it, and stay in the
   call: no new prompt (own pid filtered, call handled while recording).
   Why skipped: needs a real machine, a browser call and audio devices.
4. Linux (Ubuntu 24.04, PipeWire with pipewire-pulse; and once on a plain
   PulseAudio desktop): repeat 1 and 3 with the Zoom `.deb` (process `zoom`).
   Run `pactl list source-outputs` during the call.
   Expect: one prompt for Zoom; the source output lists
   `application.process.binary = "zoom"`. With `RUST_LOG=audio=debug`, the
   log shows `apps using a mic` with `zoom` in it and never meet-ai.
   Why skipped: needs a real Linux desktop with a sound server and Zoom.
5. Linux, Slack open all day, no call: no prompt. Start a Slack huddle.
   Expect: one "Slack is open." prompt after about 20 s.
   Why skipped: needs a real desktop and a Slack account.
6. Linux, sound server stopped (`systemctl --user stop pipewire-pulse
   pulseaudio`): start meet-ai.
   Expect: no sound server is started by meet-ai, no prompts, one warning
   "could not read audio device activity" in the log, and the app is fine.
   Why skipped: needs a real Linux desktop.
