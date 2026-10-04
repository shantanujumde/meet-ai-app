# Manual checks: tur91

TUR-91: the Mac's sound got quieter when a recording started. Cause: with
Bluetooth buds as the default input, `audio::mic` opened the buds' mic, and
macOS switched the buds from playback mode (A2DP) to headset mode (HFP). Now,
when the default input is Bluetooth and the Mac has a built-in mic, the
built-in mic is recorded, so the buds stay in playback mode. Setting:
Settings > Files > "Use the Mac's own mic when Bluetooth headphones are
connected" (`audio.use_builtin_mic_with_bluetooth`, default on).

The choice is unit-tested with fake device lists (`audio::mic_choice`).
Nothing below could run here: each needs a signed build, the running app,
real speakers or headphones, and the owner's Bluetooth buds.

## Run by hand

Signed build of this branch (`just bundle-signed`). After each step, read
`~/Meetings/.app/logs/meet-ai.log`.

1. Bluetooth buds, setting on, no call. Play music, start a recording, talk,
   stop after 1 minute.
   Expect: the music does not get quieter or duller at start or stop. The
   log has `microphone: "MacBook Pro Microphone" instead of the default,
   because the default input is Bluetooth ...` and `microphone device rate
   48000 Hz` (not 16000). `system tap rates (start)` shows `output device
   Some(44100.0)` or `Some(48000.0)`, not `Some(16000.0)`, and `system tap
   buffers: aggregate has 1 input stream(s)` (only the tap). `mic.wav` has
   your voice.
2. Bluetooth buds, setting off. Same as 1.
   Expect: the old behaviour: sound goes to call mode while recording. The
   log says `the default input ... because the default input is Bluetooth,
   but the setting ... is off`. `system.wav` has none of your voice (TUR-87's
   tap-buffer handling still applies).
3. Bluetooth buds in a WhatsApp call, setting on. Start the call, then record.
   Expect: the buds are already in call mode because WhatsApp opened their
   mic; meet-ai changes nothing further, and stopping the recording does not
   change the sound. The built-in mic is recorded.
4. Built-in speakers and mic, no headphones. Play music, record, stop.
   Expect: no volume change. Log: `the default input ... is not Bluetooth`.
5. Wired headphones (3.5 mm or USB-C). Play music, record, stop.
   Expect: no volume change. A wired/USB headset mic is still recorded if it
   is the default input.
6. Buds connect in the middle of a recording (setting on).
   Expect: the segment reopens (`default_input_device_changed`), the log
   shows the built-in mic chosen again, and the buds stay in playback mode.
7. On a Mac with no built-in mic (Mac mini/Studio) with buds as input.
   Expect: the buds' mic is recorded and the log says `this Mac has no
   built-in mic`.

## Other causes checked (no change needed)

- Process tap: `CATapMuteBehavior::Unmuted` (`macos/tap.rs`), private tap,
  the aggregate's only sub-device is the current default output. Nothing sets
  volume or the default device.
- Voice processing / ducking: no `kAUVoiceIOOtherAudioDucking`,
  `setVoiceProcessingEnabled` or VoiceProcessingIO anywhere in `crates/`,
  `src-tauri/` or `sidecar/`.
- Chime (`audio::chime`): plays a tone through a cpal output stream; never
  changes the output volume.
