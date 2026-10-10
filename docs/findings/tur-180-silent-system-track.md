# TUR-180, problem 2: the other person went silent, and the "Others" track is all zeros

Status: **not solved, not reproduced.** This file collects what the ticket saw,
what meet-ai does at the start of a recording that touches the same Bluetooth
headset, and a step-by-step plan to find out which step (if any) is to blame.
It is for TUR-163. The crash from the same call (problem 1) is fixed by the
TUR-180 PR and is not covered here.

All file:line references are to `origin/main` at `6c43f968` (2026-10-10).

## What the ticket saw (2026-10-09, WhatsApp call, Bluetooth headset)

Times are UTC, from `~/Meetings/.app/logs/meet-ai.log` on the owner's Mac.

| Time | What happened |
| -- | -- |
| 06:11:43 | WhatsApp call detected; the prompt card is shown. |
| 06:11:48 | Recording starts. Mic: the MacBook's built-in mic (chosen on purpose, the default input was the Bluetooth headset). The system tap's output device runs at **16000 Hz**, which is the headset's call mode (HFP). |
| 06:11:50 | The system-audio check plays its chime through the headset; the tap hears it. Warning: the tap delivered `17094` frames/s against `16000` expected. |
| 06:11:53 | `system tap input rate changed 16000 Hz -> 44100 Hz mid-recording`: the headset left call mode (HFP) for music mode (A2DP). |
| ~06:11:54 | WhatsApp lets go of the mic (logged 5 s later as `off the mic for 5 s`). |
| 06:12:06 | Call-end countdown stops the recording. |

The recording's `system.wav`, measured per second with ffmpeg `astats`:
`-115, -52 (the check chime), -97, -inf, -inf, ...` (19 one-second blocks).
From second 3 to the end it is **exact digital zeros**. `mic.wav` is normal
speech the whole time. The user could not hear the other person.

So the order is: recording starts while the headset is in call mode, the
chime plays, about 3 to 5 s later the headset drops out of call mode, and from
then on nothing plays on the Mac's output at all. WhatsApp leaves the mic at
about the same moment.

Earlier rate changes of the same kind are in the log (2026-10-06 10:40 and
10:42, 2026-10-07 06:41 UTC). Whether call audio was lost then is unknown.

The first open question is whether the other person could be heard **before**
Record was pressed (06:11:43 to 06:11:48). If not, WhatsApp or the headset is
the cause, not meet-ai.

## The three suspects: what meet-ai does at the start of a recording

A recording starts in `src-tauri/src/recording.rs:285-311`: it makes the mic
source (`:285`), the system source (`:286`), starts both
(`RecordingSession::start_with_tees`, `:293`), and then starts the
system-audio check on its own thread (`check.spawn(app)`, `:311`, which runs
`audio::permission_check::during_recording::check` via
`src-tauri/src/recording/start_check.rs:106`).

### Suspect A: an aggregate device with the Bluetooth output as its main sub-device

`crates/audio/src/macos/tap.rs`, `SystemSource::build`:

- `:271-278` reads the **default output device** (here: the headset, in call
  mode at 16 kHz) and its UID.
- `:281-294` makes a global stereo process tap over every process
  (`initStereoGlobalTapButExcludeProcesses` with an empty list), private,
  with `CATapMuteBehavior::Unmuted` (`:294`), so the tapped audio should still
  play.
- `:324-357` creates a private aggregate device whose **only sub-device and
  main sub-device is that output device** (`kAudioAggregateDeviceMainSubDeviceKey`,
  `:342`), with the tap in its tap list and drift compensation on.
- The aggregate is then started, which runs I/O on the headset through the
  aggregate.

Why it might matter: the aggregate opens its own I/O on the headset while
WhatsApp has it in call mode. If Core Audio or the Bluetooth stack treats that
as a new client on the device, it could renegotiate the headset's profile,
which would match the 16 kHz -> 44.1 kHz change the tap logged
(`crates/audio/src/macos/tap_rate.rs:280`). This is a guess; it needs the
reproduction below.

### Suspect B: opening the built-in mic while the call holds the headset mic

`crates/audio/src/mic.rs:363-380` (`choose_device`) and
`crates/audio/src/mic_choice.rs:62-95` (`choose`): when the default input is a
Bluetooth device and the Mac has a built-in mic, the recording opens the
**built-in mic** instead (TUR-91, setting
`audio.use_builtin_mic_with_bluetooth`, default on). The reason given in
`mic_choice.rs:3-11` is to keep the headset out of call mode. In a call,
though, the call app already holds the headset mic and the headset is
already in call mode.

Why it might matter: a second app opening a different input while the call
app holds the headset mic is a setup that rarely happens otherwise. Whether
macOS changes the headset's mode or the call app's input when this happens is
not known; verify it.

### Suspect C: the check chime played into a headset in call mode

`crates/audio/src/permission_check/during_recording.rs:52-70` (`check`): on
macOS (where a denied system-audio grant records silence) every recording
plays the permission chime to prove the tap hears audio. The chime goes out
through `ChimeOutput::open`
(`crates/audio/src/permission_check/chime_output.rs:24-45`), which opens a new
**cpal output stream on the default output device** (the headset) at its
default output config, and plays it once, then up to `chime::MAX_PLAYS` times
if not heard.

Why it might matter: it is a new output stream on the headset at the exact
moment things went wrong (06:11:50, the rate change follows at 06:11:53). The
same check logged a pace error (`17094` frames/s against `16000`), so the
device was not running at the rate it reported. Opening an output stream on a
Bluetooth device in call mode may make macOS reconsider its profile. Verify
it.

## Reproduction plan (isolates each suspect)

Needs: a Mac, the same kind of Bluetooth headset (set as both default input
and output), a second phone or account for a WhatsApp (or FaceTime) call, and
a dev build of meet-ai. One person on the call speaks or plays music the whole
time. For each run, write down: can you hear the other person (before and
after), the headset rate lines in `meet-ai.log`
(`system tap input rate changed`), and the `system.wav` loudness per second
(`ffmpeg -i system.wav -af astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level -f null -`).

0. **Baseline, no meet-ai recording.** Start the call, wait 60 s. Confirm the
   other person is heard the whole time and the headset stays in call mode
   (Audio MIDI Setup shows the headset output at its call rate, 16000 Hz for
   the ticket's headset, not its music rate, 44100 Hz there). If the call goes
   silent here, stop: it is not meet-ai.
1. **Full recording, as in the ticket.** Start the call, wait 10 s, press
   Record. Expect the ticket's result if meet-ai is the cause. Repeat 3 times;
   note how many go silent.
2. **Without the chime (suspect C off).** Record with the CLI, which starts the
   same mic and the same tap and aggregate but never plays the check chime:
   `cargo run -p audio --bin meet-rec -- --out <dir> --duration 60` during the
   call. If the call stays audible here but not in run 1, suspect C is the
   cause.
3. **Without the aggregate (suspect A off).** A dev build with the system
   source turned off (in `src-tauri/src/recording.rs:286`, a local patch
   `let sys = None;`; do not commit it). The chime check then has no tap and
   reports itself unmeasurable; to also keep the chime off, skip
   `check.spawn(app)` at `:311` in the same patch. If the call stays audible,
   suspect A (or A with C) is the cause; compare with run 2.
4. **Mic choice (suspect B).** Settings: turn off "Use the Mac's own mic when
   Bluetooth headphones are connected" (`audio.use_builtin_mic_with_bluetooth`), so
   the recording opens the headset mic the call already holds, and repeat run 1.
   Then run 3's patch with the setting back on (mic only, built-in). If only
   the built-in-mic runs go silent, suspect B is the cause.
5. **A wired or built-in output.** Repeat run 1 with the Mac's speakers or wired
   headphones as output and the headset not connected. If the call stays
   audible, the problem is specific to Bluetooth call mode.

## Recommended fix per suspect (once the reproduction points at one)

- **A (aggregate on a headset in call mode):** before building the tap, read
  the output device's transport and nominal rate (the code already reads rates
  in `crates/audio/src/macos/tap_rate.rs`). If it is a Bluetooth device in call
  mode (running at its call rate, 16000 Hz in the ticket; verify the rates
  other headsets use) and a call app is on the mic (detection
  already knows), try making the aggregate with the tap only and no sub-device
  as the main clock, or delay the system track until the call app leaves the
  mic, and log it. If none of these keep the call audible, record the mic only
  in that case and say so in the meeting's status rather than recording zeros.
- **B (built-in mic while the call holds the headset mic):** when a call app is
  already using the headset mic, the TUR-91 reason no longer applies (the
  headset is already in call mode). Keep the built-in choice only when no
  other app holds the Bluetooth mic; otherwise open the default input.
- **C (check chime in call mode):** do not play the chime while another app is
  on the mic (a call is going on), or while the output is a Bluetooth device in
  call mode. The check can wait for the first non-zero system audio instead
  (TUR-84 already accepts real non-zero audio as proof of the grant), and only
  fall back to the chime once the call has ended or after a longer wait.
- **Whatever the cause:** when the system track turns to exact zeros
  mid-recording while the mic still has speech, log it once and show it in the
  recording status, so the user knows within seconds rather than after the
  meeting.
