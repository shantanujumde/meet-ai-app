# Manual checks: tur142

TUR-142: `audio::mic_users::mic_users()` lists the apps using a mic, by name,
never meet-ai itself. macOS 14+ reads Core Audio's process list
(`crates/audio/src/macos/activity.rs`), Windows the WASAPI capture sessions
and Linux the PulseAudio source outputs (`crates/audio/src/platform/*/activity.rs`).
The naming and filtering are OS-free (`crates/audio/src/mic_users/`) and
unit-tested with fake process data on every OS. Nothing in the app calls it
yet (TUR-143 and TUR-144 will), so detection behaviour is unchanged.

Every check below uses the example that prints the list every 2 s:

```sh
cargo run -p audio --example mic_users 120
```

## Run by hand

1. macOS 14+: start a Zoom call, then a Google Meet call in Chrome, a WhatsApp
   call and a FaceTime call (one at a time). Hang up each.
   Expect: while the call is up, a line with `Zoom [CallApp]`,
   `Google Chrome [Browser]`, `WhatsApp [CallApp]` or `FaceTime [CallApp]`;
   after hang-up the app is gone from the next line (2 s).
   Why skipped: needs real calls, a mic and signed-in meeting apps.
2. macOS: join a call in Arc, in Helium and in Safari.
   Expect: `Arc [Browser]`, `Helium [Browser]`, `Safari [Browser]` (Safari's
   capture runs in `com.apple.WebKit.GPU`, named through
   `responsibility_get_pid_responsible_for_pid`).
   Why skipped: needs real calls in those browsers.
3. macOS: start a recording in meet-ai (signed build), with nothing else using
   the mic, and run the example at the same time.
   Expect: the example is a separate process, so it shows the recording app
   as `meet-ai [IgnoredSystem]` (never as a call). Inside the app, where
   TUR-143 will call `mic_users()`, the list is empty: the app's own pid and
   its WebKit child (whose responsible process is the app) are dropped. Once
   TUR-143 lands, check that a recording alone never counts as a call.
   Why skipped: needs a signed build recording with mic permission.
4. macOS 13 or older: run the example.
   Expect: `not supported here` on every line, no error, no crash.
   Why skipped: no Mac older than 14 here.
5. Check `IsRunningInput` turning on and off: in Voice Memos, start and stop
   a recording.
   Expect: `Voice Memos [Other]` appears while recording and is gone within
   one line after it stops.
   Why skipped: needs mic permission for Voice Memos and a person to press
   record.
6. Windows 10/11: a Zoom desktop call, then a Meet call in Chrome and in Edge.
   Expect: `Zoom [CallApp]`, `Google Chrome [Browser]` (Chrome records in a
   `--type=utility` audio process, named after its parent), `Microsoft Edge
   [Browser]`; each gone within 2 s of hang-up. A new Teams call should show as
   `Microsoft Teams` (its WebView2 audio service is two parents below
   `ms-teams.exe`): verify it.
   Why skipped: needs a real Windows machine and calls; CI only builds and
   runs the unit tests.
7. Linux (PulseAudio or PipeWire with pipewire-pulse): a Zoom call and a Meet
   call in Chrome and Firefox.
   Expect: `Zoom [CallApp]`, `Google Chrome [Browser]`, `Firefox [Browser]`.
   With no sound server running, `not supported here`.
   Why skipped: needs a Linux desktop with a sound server and calls.

## Seen on this Mac (headless, no calls)

- `cargo run -p audio --example mic_users 3` printed `nobody else`: nothing
  was using the mic, and the read raised no prompt.
- A throwaway test (not committed) that named every process in Core Audio's
  list, not only those using the mic, gave sensible names: Slack and Helium
  helpers as `Slack` and `Helium`, a WebKit GPU process as the app
  responsible for it (`Raycast`, `Kiro CLI`), `com.apple.avconferenced` as
  `FaceTime`, `callservicesd` as `Phone call`, `corespeechd`
  (`com.apple.CoreSpeech`) and Siri as `Siri [IgnoredSystem]`, and
  daemons such as `audiomxd` as `IgnoredSystem`. meet-ai's own WebKit GPU
  process had the running meet-ai as its responsible process.

## Decisions taken

- The answer is `MicUsers::Supported(Vec<MicApp>)` or `NotSupported`; a read
  failure (no sound server, WASAPI error) is `NotSupported`, logged at debug,
  never shown to the user.
- `MicApp.pid` is the process that has the mic open (a browser's helper), not
  the browser's main process.
- macOS programs under `/System/Library/`, or outside any app bundle in
  `/System/`, `/usr/bin|libexec|sbin/` or `/Library/Apple/`, that the table
  does not name are `IgnoredSystem`: system daemons, never a call. Apple's
  apps in `/System/Applications/` (Voice Memos) stay `Other`.
- A second copy of meet-ai (`pro.saleschat.meetai`, another pid) is
  `IgnoredSystem`; the running app's own pid is dropped before naming.
- Windows parent lookups follow at most three parents (enough for a WebView2
  app's audio service), so a cycle in reported parents cannot loop.
- BlackHole is a driver, not a process, so it needs no table entry.
