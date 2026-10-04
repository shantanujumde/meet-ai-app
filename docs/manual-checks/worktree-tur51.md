# Manual checks: TUR-51 (permission check, onboarding and settings links on Windows and Linux)

These need a real Windows PC or Linux desktop with a microphone and speakers,
or the running app, so none were run here. Headless coverage:

- `crates/audio/src/platform/windows_consent.rs` tests: the registry decision
  (`mic_consent(hklm, hkcu, nonpackaged)`, including the debloater case) and
  the `E_ACCESSDENIED` text match. They run in every `cargo test -p audio`,
  macOS included.
- `src-tauri/src/permission.rs` tests: `NotApplicable` counts as allowed, a
  denied mic still refuses, and the refusal names each OS's settings app.
- `src-tauri/src/settings_links.rs` tests: the settings target per OS and
  desktop, and that every URL scheme is in the opener scope.
- `src/lib/osText.test.ts`, `src/lib/recordingPermission.test.ts`
  (`permissionStepShown`), `src/ipc/errors.test.ts`,
  `src/routes/onboarding/PermissionStep.test.tsx`: wording and step choice per OS.

The Windows file (`windows_permission.rs`) was only type-checked here
(`cargo check --target x86_64-pc-windows-msvc -p audio`). The Linux file
(`linux_permission.rs`) was not compiled here at all (no Linux target on this
Mac). The `rust (windows)` and `rust (linux)` CI jobs build and test both.

## 1. Windows: Granted with the microphone on

1. On a Windows 10/11 PC, Settings → Privacy & security → Microphone: turn on
   **Microphone access** and **Let desktop apps access your microphone**.
2. Run the app build from the PR's CI (or `pnpm tauri dev` on that PC), finish
   onboarding.
3. Expected: onboarding has **no** permission step. Press Record: you hear the
   start chime once, the recording starts, and the log has
   `permission check before recording` with `state=Granted`.

## 2. Windows: Denied with the microphone off

1. Same page, turn off **Let desktop apps access your microphone**.
2. Open onboarding again (Settings → setup) or press Record.
3. Expected: the permission step shows "Windows is blocking the microphone",
   Record is disabled, the error names Windows Settings, not System Settings.
4. **Open Microphone settings** opens Settings → Privacy & security →
   Microphone. Turn the switch back on, **Check again**: Allowed.
5. Repeat with only **Microphone access** (the device switch) off: Denied too.
6. Debloater case: set
   `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone`
   `Value` to `Deny` while `...\microphone\NonPackaged` stays `Allow`.
   Expected: Allowed, and a recording captures your voice.

Parity gate, unverified: whether a Windows build returns silence instead of
`E_ACCESSDENIED` when the switch is off and the registry read missed it. With
step 2's switch off, also delete the `NonPackaged` `Value` and press Record.
Expected: Denied ("Windows refused the microphone (E_ACCESSDENIED)" in the
log). If it records silence instead, file a follow-up.

## 3. Linux: allowed

1. On a GNOME and on a KDE desktop, run the app and finish onboarding.
2. Expected: no permission step; Record starts with the start chime.
3. Unplug or disable every input device and press Record. Expected: a device
   error ("no audio device available…"), never the permission screen.
4. Where a settings button is shown (Settings pane), **Open Sound settings**
   opens `gnome-control-center sound` on GNOME and `systemsettings
   kcm_pulseaudio` on KDE. On another desktop (XFCE) it does nothing and logs
   that no settings page is known.

## 4. macOS: unchanged

1. Signed build on macOS 26: onboarding still shows both grants and both
   buttons; Record plays the chime once (as the system-audio check), not twice.
2. Deny Microphone in System Settings: the denied screen and the "System
   Settings" refusal text are as before.
