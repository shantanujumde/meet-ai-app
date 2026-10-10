# Manual checks: tur180

TUR-180, problem 1: on macOS the app aborted (`abort()` from
`__rust_foreign_exception` in tao's run loop observer) 0 to 3 s after every
recording stop, on the build with the TUR-146 overlay. The overlay is
TUR-147's panel (its class swapped to an `NSPanel` subclass under tao), and
every stop closed it. The prompt card has the same swap but is never closed,
and never crashed.

The fix (SPEC A34):

- On macOS the overlay window is made for the first recording, then hidden
  (`orderOut:`) when a recording ends and shown again for the next one, at
  the saved spot. It is never closed while the app runs
  (`overlay::platform::KEEP_WINDOW`; the choice is `overlay::step`, unit
  tested). Windows and Linux still close it.
- Its position is saved before it is hidden or closed, as before.
- Our own AppKit calls (`become_panel`, show, hide) run inside
  `objc2::exception::catch`; an Objective-C exception is logged at error level
  with its name and reason (`AppKit threw an Objective-C exception`) instead
  of aborting with no message.
- The panel class answers `canBecomeKeyWindow` from tao's `focusable` ivar, so
  the overlay (built `focusable(false)`) never becomes the key window. The
  prompt card (focusable) keeps today's answer.

Problem 2 (the silent "Others" track) is written up in
`docs/findings/tur-180-silent-system-track.md` for TUR-163; no audio code
changed.

Ran headless here: `cargo test -p meet-ai --lib` for `overlay` and
`detection::popup` (step choice, position save and restore, the focusable
ivar's offset and answer, an NSException caught by `catching`), clippy,
fmt, and the quality gate. Nothing below ran: no app was launched, no window
was made, no audio was recorded.

## Run by hand

Use a build of this branch on macOS (the ticket's Mac: macOS 27, two screens).

1. **The crash is gone.** Run the app under lldb with
   `br set -n objc_exception_throw`. Start a recording (the overlay appears),
   wait 10 s, stop it. Do this 3 times in one app session.
   Expect: no breakpoint hit, no crash, the meeting saves (`transcript.md`
   and `notes.md` are not 0 bytes, no `.incomplete` left in `audio/`).
   If the breakpoint does hit, `po $x0` (or `bt`) shows the exception and
   what threw it; note it on the ticket.
   Why skipped: needs the running app and a window server.
2. **To confirm the cause (optional).** The same on `main` before this fix:
   expect the breakpoint to hit inside the overlay close on the first stop.
   Why skipped: as 1.
3. **The overlay comes back where it was left.** Drag the overlay to another
   spot (also try the other screen), stop, start again.
   Expect: it reappears at the same spot each time, showing the new
   recording's timer from `00:00` and none of the last recording's words
   (note any brief flash of the old words).
   Why skipped: needs a window server.
4. **It never takes the keyboard.** In a call app (or TextEdit), start a
   recording, click on the overlay's card and on Pause, then type.
   Expect: the typing still goes to the other app; Pause, Resume and Stop
   work on the first click; the overlay is not in ⌘\` window cycling.
   VoiceOver can still read the overlay's buttons (note if it cannot).
   Why skipped: needs a window server.
5. **Hidden overlay and quit.** After a recording has stopped (overlay
   hidden), quit from the menu bar, and in a second run with ⌘Q.
   Expect: a clean quit, no crash note in `~/Meetings/.app/logs/`.
   Why skipped: needs the running app.
6. **Setting off and on mid-recording.** Settings, "Show the recording
   window over other apps" off during a recording: the overlay goes away at
   once; on again: it comes back. Stop with it off, start again: no overlay.
   Why skipped: needs a window server.
7. **Full-screen call still works.** A full-screen Zoom or Meet call,
   start a recording: the overlay shows over it (TUR-146 check 2), on the
   first and on a later recording.
   Why skipped: needs a meeting app in a full-screen Space.
8. **Windows and Linux unchanged.** Start and stop twice: the overlay closes
   on stop and opens again on start, at the saved spot.
   Why skipped: needs real Windows and Linux machines (CI builds and tests
   them).

## Follow-ups noticed

- `src-tauri/src/lib.rs:101` (single-instance handler) focuses
  `app.webview_windows().values().next()`, which can be the prompt card or,
  now that it is kept on macOS, the hidden overlay, rather than the main
  window. Not changed here (shared file, not this ticket's); worth a small
  ticket to look the main window up by label.
