# Manual checks: Wave 3 Phase 3 (split backend files, rename commands)

All of this is a pure move plus one rename, so the checks below confirm the
running app behaves as before. None of them was run headless.

1. **Record and stop a meeting** (`just dev`, press Record, speak, press Stop).
   Expect: state changes show in the window and tray; `transcript.md` fills in
   as you speak; the live pane and status line update. Skipped: needs mic,
   system audio and the running app.
   **Passed 2026-10-01** on a signed bundle. Window: Stop button, timer and a
   "Recording" tag in the list. Tray: the menu item reads "Stop recording"
   while recording. The tray icon itself never changes (by design, see
   `tray.rs`), so nothing in the menu bar shows a recording is running unless
   the menu is opened. `transcript.md` matched the live pane.
2. **Permission check on the onboarding / Settings screen.** The command is now
   `measure_permission` (it still plays the chime). Expect: same chime and same
   granted/denied readout as before. Skipped: needs a macOS permission dialog.
   **Passed 2026-10-01** on a signed bundle (`just bundle-signed`), after
   `tccutil reset Microphone pro.saleschat.meetai`. Denied: "NOT ALLOWED" with
   the `AVAuthorizationStatusDenied` detail, system audio still read correctly
   from the tone, fix-it card with both System Settings buttons and Check again.
   Switched on in System Settings, then Check again: chime and "ALLOWED".
3. **Shortcut refusal.** Press ⌘⇧R while a folder move is running. Expect: the
   "did not start recording" notification (now in `notify.rs`). Skipped: needs
   the running app and notification permission.
4. **Interrupted recording notice.** Kill the capture mid-meeting if you can
   (unplug the input device). Expect: "meet-ai stopped recording" notification.
   Skipped: needs a device.
   **Not reachable by unplugging (2026-10-01).** The notice fires only when a
   tick fails (`recording.rs`, `fail_mid_recording`), and unplugging a device
   does not fail a tick: the app reopens on the new default device and keeps
   going. Unplugging a 3.5 mm headset mid-recording: no notice, recording kept
   running, all three spoken parts in the transcript. Unplug gave 1 reopen
   (`default_output_device_changed`); replug gave 2 reopens 1.5 s apart (output,
   then input). Lost at the switches, over a 49.2 s span: about 0.6 s of mic,
   about 2.5 s of system audio. Still open: a way to make a tick fail for real.
5. **Meetings list with a cut-short recording.** Open the list with a meeting
   whose WAV has no finished header. Expect: it is listed as interrupted and the
   audio is repaired as before (code moved to `audio::wav_repair`). Skipped:
   needs a real interrupted recording.
   **Passed 2026-10-01**, using `kill -9` on the signed bundle mid-recording,
   then relaunching it:
   - Killed at 12 s (after checkpoints): the headers covered 10.3 of the 12.0 s
     on disk, `segments.json` had 2 anchors. Listed as INTERRUPTED with its 1
     transcript line. The headers are not raised, by design: only a meeting
     with no `segments.json` is repaired, so the 1.7 s past the last checkpoint
     is dropped.
   - Killed at 2 s (before the first checkpoint, no `segments.json`): the
     headers declared 0 bytes. Listed as INTERRUPTED. On the next launch's list
     both headers were raised to cover all the audio (2.60 s / 2.51 s), the
     samples were unchanged byte for byte, and ffprobe reads the files.
6. **Ignored live-transcript end-to-end tests** (`cargo test -p meet-ai --lib
   live_transcript::e2e -- --ignored --nocapture --test-threads=1`, after
   `just fixtures`). They need a whisper model or the Apple speech model.
   Skipped: model download / sidecar.

Separate follow-up (not part of this task): flip rule R2 to ERROR once a human decides.
