# Manual checks: tur84

TUR-84: on Bluetooth buds the system-audio check said Denied although real
audio came back, and the tap resampled from the input stream's claimed 48 kHz
while frames arrived at the aggregate's 16 / 44.1 kHz. Now real (non-zero)
audio always counts as granted, the device nominal rates win over the stream
format, and the delivered rate is measured from frames vs `mHostTime` (it wins
when it is more than 2 % off). All of it is unit-tested headless with fake
feeds and a fake device clock; nothing below could run here because it needs
real speakers, a Bluetooth headset, a signed build and the running app.

## Run by hand

Signed build of this branch (`just bundle-signed`), both System Settings
switches on. After each, read `~/Meetings/.app/logs/meet-ai.log`.

1. Buds in A2DP (play music first). Settings → Check again.
   Expect: system audio Granted. The log's `system tap rates` lines show
   `effective_rate 44100 Hz` (or the buds' real output rate, as in
   `system_profiler SPAudioDataType`), and a `system tap rates (measured)`
   line whose `measured_rate` is within 2 % of it. If the chime was not
   recognised, a warning "system audio is flowing but the check tone was not
   recognised" carries the rates; the check is still Granted.
   Then record 1 minute while music plays, stop, and compare durations:
   `afinfo system.wav mic.wav | grep duration`.
   Expect: equal to within a few tens of ms.
2. Buds in a WhatsApp call (HFP, 16 kHz). Start the call, then Check again,
   then record 1 minute of the call.
   Expect: Granted; `effective_rate 16000 Hz`; `system.wav` duration equals
   `mic.wav`; the other side's speech appears in the live transcript at normal
   pitch and speed. Starting the call mid-recording should log
   `rate changed`, `system tap resampler switched`, and keep the durations
   equal.
3. Built-in speakers. Check again, record 1 minute.
   Expect: Granted with the chime heard (`present=true`); `effective_rate
   48000 Hz` (or the speakers' rate), no "delivers ... Hz, not the ... Hz in
   use" warning; durations equal.
4. Denial still works: turn the system-audio switch off, Check again.
   Expect: Denied, log `peak=0 rms=0 ended="window"`.

Why skipped: each needs real output hardware, the TCC grant of a signed
build, and the running app, none of which this headless worktree has.

## Known limits

- If every reported rate is wrong, the first ~0.5 s of a segment is
  resampled at the wrong rate before the measurement lands. Its duration is
  put right (silence added, or excess frames dropped from what follows), but
  its audio stays pitch-shifted. With the owner's buds the aggregate's
  nominal rate is right, so this path is only a safety net.
- A rate change with no Core Audio notification is only picked up after two
  0.5 s windows agree, about 1 s late.
