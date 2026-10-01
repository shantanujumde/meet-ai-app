# Manual checks: Wave 3 Phase 3 (split backend files, rename commands)

All of this is a pure move plus one rename, so the checks below confirm the
running app behaves as before. None of them was run headless.

1. **Record and stop a meeting** (`just dev`, press Record, speak, press Stop).
   Expect: state changes show in the window and tray; `transcript.md` fills in
   as you speak; the live pane and status line update. Skipped: needs mic,
   system audio and the running app.
2. **Permission check on the onboarding / Settings screen.** The command is now
   `measure_permission` (it still plays the chime). Expect: same chime and same
   granted/denied readout as before. Skipped: needs a macOS permission dialog.
3. **Shortcut refusal.** Press ⌘⇧R while a folder move is running. Expect: the
   "did not start recording" notification (now in `notify.rs`). Skipped: needs
   the running app and notification permission.
4. **Interrupted recording notice.** Kill the capture mid-meeting if you can
   (unplug the input device). Expect: "meet-ai stopped recording" notification.
   Skipped: needs a device.
5. **Meetings list with a cut-short recording.** Open the list with a meeting
   whose WAV has no finished header. Expect: it is listed as interrupted and the
   audio is repaired as before (code moved to `audio::wav_repair`). Skipped:
   needs a real interrupted recording.
6. **Ignored live-transcript end-to-end tests** (`cargo test -p meet-ai --lib
   live_transcript::e2e -- --ignored --nocapture --test-threads=1`, after
   `just fixtures`). They need a whisper model or the Apple speech model.
   Skipped: model download / sidecar.

Separate follow-up (not part of this task): flip rule R2 to ERROR once a human decides.
