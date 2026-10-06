# Manual checks: TUR-121 (system audio comes back at the next device change)

Skipped here: needs a signed build, real earbuds and a meeting app playing audio.

1. Signed build, start a recording with speakers as output and audio playing.
   Connect Bluetooth earbuds mid-recording, then disconnect them.
   Expected: in the meeting's `audio/segments.json`, every segment after a
   device change has `sys_rate` > 0, including one that follows a segment
   where `sys_rate` was 0 (tap dropped). Others lines keep arriving in the
   live transcript after the tap comes back.
2. Start a recording while system-audio capture fails at start (e.g. permission
   revoked), then grant it and switch output device. Expected: the next
   segment has `sys_rate` > 0. Verify it: whether macOS lets the tap start
   again without an app restart.
