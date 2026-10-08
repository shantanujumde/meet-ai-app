# Manual checks: tur146

TUR-146: Pause / Resume for a recording, and a small always-on-top overlay
window (label `overlay`, about 320×90) while recording: the timer (paused
time left out), the last line or two of the live transcript, Pause or Resume,
and Stop. Clicking the text brings the main window forward. The main window's
`RecordControl` gets the same Pause button. Settings: "Show the recording
window over other apps" (`audio.show_recording_overlay`, on by default).
SPEC A33.

How it works, in short: a pause flips a switch (`audio::session::PauseSwitch`)
that the ticker thread applies at its next tick. Both channels stop as for a
segment reopen (`segments.json` written at once); resume opens fresh sources
onto the same WAVs with a `resumed_after_pause` segment. The phase stays
`recording`; `recording_status.pause` carries `pausedAtMs` / `pausedTotalMs`
for every window's timer. The overlay follows the recorder (`overlay/mod.rs`
worker on every recording-state event) and, on macOS, is TUR-147's
non-activating panel (`detection::popup::make_panel` / `show_floating`).
TUR-145's silence stop holds its ten minutes at zero while paused.

Tested headless on a Mac:

- `cargo test -p audio -p meeting-format`: a pause writes nothing and a
  resume carries on in the same files (with a `resumed_after_pause`
  segment), stopping while paused finishes the files as the pause left them,
  a system-audio drop asked for during a pause is honoured at the resume, and
  a mic that will not restart fails the resume and stays paused (fake
  sources).
- `cargo test -p meet-ai` (735 passed): `recording::pause` (the switch, the
  clock, only a live recording pauses, stop and the next start clear it, the
  camelCase wire shape), `overlay::` (when the overlay is wanted, default
  spot, saved spot kept or dropped when off every screen, `state.json` key),
  `config::audio_overlay`.
- `cargo clippy -p meet-ai -p audio -p meeting-format --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `cargo check --target x86_64-pc-windows-msvc -p audio -p meeting-format`.
- vitest, one file at a time: `src/lib/elapsed.test.ts`,
  `src/state/recording.test.ts`, `src/ui/Overlay.test.tsx`,
  `src/ui/RecordControl.test.tsx`, `src/ui/OverlaySetting.test.tsx`,
  `src/routes/Settings.test.tsx`, `src/ui/Shell.scroll.test.tsx`; `pnpm typecheck`.

Note: `src/state/recording.test.ts` used to hang at 0% CPU. Its
`@/ipc/client` mock factory awaited `import("@/ipc/recordingPause")`, which
imports `./client`, the module being mocked, so the import never settled.
`isPaused` now lives in `src/lib/elapsed.ts` (pure), and the mock imports it
from there.

Nothing below ran: no app was launched, no window was made, no audio was
recorded.

## Run by hand (Wave H)

Use a signed build of this branch on macOS 26, and builds on Windows 11,
Linux X11 and GNOME Wayland.

1. Start a recording from the main window. Switch to another app.
   Expect: the overlay appears top-right, under where the prompt card goes,
   over the other app; the timer counts `00:01`, `00:02`; the latest words
   show within a second or two of being said. meet-ai does not become the
   active app, and the overlay is not in the Dock, the taskbar, ⌘-Tab /
   Alt-Tab, or ⌘\` window cycling.
   Why skipped: needs the signed app, a mic and a window server.
2. macOS: make a Zoom or Meet call full screen and start a recording (⌘⇧R).
   Expect: the overlay shows over the full-screen call, and typing in the
   call's chat keeps going to the call after clicking Pause on the overlay.
   Verify it: the log line `the prompt card is a non-activating panel` is
   written when the overlay opens (the same helper as the prompt card).
   Why skipped: needs a real meeting app in a full-screen Space.
3. Drag the overlay by its card to the bottom-left. Stop, then record again.
   Expect: it opens where it was left. Unplug the external screen it was on
   (or move it there and disconnect): it opens top-right again.
   Why skipped: needs a window server and a second screen.
4. Pause from the overlay for 20 s while speaking, then Resume, speak, Stop.
   Expect: the timer freezes at the pause and carries on from the same value;
   the dot stops pulsing and the main window reads "Paused"; no transcript
   lines for those 20 s; the macOS mic indicator goes off while paused (verify
   it); one meeting in the list; `mic.wav` / `system.wav` are about 20 s
   shorter than the wall-clock time; `segments.json` has a second segment
   with reason `resumed_after_pause`.
   Why skipped: needs a real mic and speakers.
5. Pause for more than 10 minutes, then Resume.
   Expect: no "No one has spoken for 10 minutes" card during the pause; the
   ten minutes start again on resume.
   Why skipped: needs a real recording.
6. Pause from the main window, Resume from the overlay, Stop from the main
   window. Expect: both windows agree at every step, and Stop closes the
   overlay and saves the meeting.
7. Click the transcript text on the overlay with the main window behind
   another app or minimised. Expect: the main window comes to the front.
8. Settings → "Show the recording window over other apps" off during a
   recording: the overlay closes at once; on again: it opens. Off, then start
   a recording: no overlay.
9. Windows 11: the card has the acrylic blur and rounded transparent corners,
   no 1 px frame. Linux X11: the card is the plain CSS fill and stays on top.
   GNOME Wayland: note whether the overlay stays on top and where it opens;
   the compositor may ignore both (best effort, not a blocker).
   Why skipped: needs real Windows and Linux machines.
10. Pause right after Start, in the first seconds (while the system-audio
    check of A25 is still listening). Expect: no "System audio is off"
    warning caused by the pause; if it shows, note it (the check calls a
    stalled tap unmeasurable, which should change nothing).
