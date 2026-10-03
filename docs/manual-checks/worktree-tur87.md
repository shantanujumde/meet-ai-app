# Manual checks: tur87

TUR-87: recording robustness on Bluetooth and USB headsets. The system tap
now reads only its own IO buffers (H1), a system track that is slow or
silent leaves the recording microphone-only instead of ending it (C3), only
a denied microphone refuses Record (H2), the microphone measures its own
delivered rate (M1), the live drift guard judges each track against the
host clock (M3), the permission check times its chime by wall clock (M4),
and `segments.json` records both device rates (L4). All of it is
unit-tested headless with fake sources, fake buffer lists and fake clocks.
Nothing below could run here: each needs a signed build, the running app,
real speakers and the owner's Bluetooth buds.

## Run by hand

Signed build of this branch (`just bundle-signed`), both System Settings
switches on unless a step says otherwise. After each, read
`~/Meetings/.app/logs/meet-ai.log` and the meeting's `segments.json`.

1. Buds in A2DP (play music first). Record 1 minute while music plays and you
   talk, stop.
   Expect: `afinfo system.wav mic.wav | grep duration` equal to within a few
   tens of ms. The log has one `system tap buffers:` line and one
   `system tap IO buffers: mNumberBuffers N, mNumberChannels per buffer [...]`
   line; note N and the channel counts (the device check for H1). The
   `microphone device rate ... Hz` line names the mic's rate. `segments.json`
   has `mic_device_rate` and `sys_device_rate` set.
2. Buds in a WhatsApp call (HFP). Start the call, then record 1 minute of it.
   Expect: durations equal, and none of your own voice in `system.wav`
   ("Others") beyond what the far side echoes back: play `system.wav` alone.
   The IO-buffer log line should show more than the tap's buffers
   (the headset mic), and `reading buffers a..b` should cover only the last
   one(s). The other side appears in the live transcript at normal pitch.
3. Start the WhatsApp call mid-recording (A2DP → HFP), and end it again.
   Expect: the recording keeps going through both switches; no `microphone
   track is not keeping pace` warning unless the mic really switched rate,
   in which case `microphone delivers ... Hz, not the ... Hz in use` follows
   and the durations still match.
4. Connect and disconnect the buds mid-recording (default-device change).
   Expect: the recording never stops. If the new tap is slow, the log says
   `system audio produced no audio ...; recording microphone only` and the
   new segment in `segments.json` has `sys_rate: 0`; `mic.wav` keeps growing
   and its header is patched (`afinfo` reads it).
5. System audio switch off, microphone on. Press Record.
   Expect: the recording starts (mic only), the banner says "System audio
   is off: ... recordings capture only your microphone", Record is enabled,
   and there is no `system.wav`.
6. Microphone switch off. Press Record.
   Expect: refused with "Microphone is switched off for meet-ai in System
   Settings, so it cannot record you."; Record disabled.
7. Permission check log (any of the above): `system-audio permission check
   finished` carries `captured_frames_per_s` near 16000. A value far off
   comes with the warning "a rate error, not a permission answer".

Why skipped: each needs real output and input hardware, the TCC grants of a
signed build and the running app, none of which this headless worktree has.

## Not done here

- M2 (rate follows notifications late): TUR-84 already derives the rate from
  the IO proc's own frames vs `mHostTime` and uses the output device's rate in
  the decision. Not done: a `kAudioStreamPropertyVirtualFormat` listener and
  tagging ring samples with their rate epoch.
- M5 (Record waits up to ~10 s on the permission check): split into a
  follow-up ticket. It changes the start flow (the chime would run on the
  session's own system tee, which the live transcript also reads), and a
  shorter pre-record check would now risk a false system "Denied", which
  since H2 records microphone-only.
- A system track dropped at a reopen is not retried at the next device
  change; the rest of that meeting is microphone-only (as before for a tap
  that failed to start).
