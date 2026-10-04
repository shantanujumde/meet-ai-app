# Manual checks: tur50

TUR-50 (tests and fixtures that run without macOS `say`) and TUR-96 (the
flaky fake-CLI test on Linux CI), in one PR.

What changed:

- `crates/audio/fixtures/{silence-30s,room-tone-30s}.wav` and
  `two-speaker-60s/{mic,system}.wav` (+ its JSON) are committed, made once on
  this Mac with `generate.sh` (macOS `say` + ffmpeg). `crates/stt/tests/`
  no longer generates them on first use, and `silence.rs` / `live.rs` lost
  their `#![cfg(target_os = "macos")]`. The Apple-engine tests in them keep
  skipping themselves (SKIPPED line) where `meet-stt` is not built.
- `crates/audio/tests/golden_bytes.rs` builds its tone without `sin`, so its
  hashes hold on every OS; it lost its macOS gate.
- `src-tauri/src/sync/tests.rs` uses `test_support::FakeCli` instead of
  `/bin/sh` scripts; `skip_without_fake_cli!` is now a no-op, so the
  agent-run, agent-setup and sync tests run on Windows too.
- TUR-96: `FakeCli::install` makes the copy in a `fake-cli` child process.
  Cause: the test process wrote the copy itself, a fork on another test thread
  inherited the still-open write handle, and Linux refused to exec the copy
  with `ETXTBSY` ("Text file busy"; CI run 37185732769 shows it in
  `an_installed_copy_reads_its_own_settings`, run 37183906108 as
  `codex_printing_garbage_is_invalid_json` getting the wrong error).
- New `scripts/gates/tur97-kill/gate-linux.sh` and `gate.ps1`.

macOS-only test files left, each for a real reason:
`crates/audio/tests/permission_check.rs` (TCC) and `system_closed_loop.rs`
(Core Audio tap + TCC). `windows_loopback.rs` stays Windows-only (WASAPI).
The Apple-engine tests inside the stt test files need the Swift sidecar and
skip without it.

## Run by hand

1. **Linux kill gate.** On a Linux desktop session with the .deb installed
   and meet-ai not running:
   `scripts/gates/tur97-kill/gate-linux.sh --play crates/audio/fixtures/two-speaker-60s/system.wav`
   Expect: `7/7 passed` (check 7 skipped), evidence in
   `target/tur97-kill-gate/<time>-linux-kill9-60s/`. Also run once with
   `--seconds 3600`. Not run here: it needs a Linux machine, a desktop
   session, the installed app and its microphone. Measure it: whether the
   .deb puts the program at `/usr/bin/meet-ai` (otherwise pass `--app`), and
   whether `--toggle-recording` starts a recording under both X11 and
   Wayland.
2. **Windows kill gate.** On Windows with the NSIS install and meet-ai not
   running, in PowerShell from the repo root:
   `pwsh scripts/gates/tur97-kill/gate.ps1 -Play crates\audio\fixtures\two-speaker-60s\system.wav`
   (or `powershell -ExecutionPolicy Bypass -File ...` on Windows PowerShell
   5.1). Expect `7/7 passed` (check 7 skipped). Also once with
   `-Seconds 3600`. Not run here: needs a Windows machine and the installed
   app; there is no PowerShell on this Mac, so the script was not even
   parsed. Measure it: the install path (default
   `%LOCALAPPDATA%\meet-ai\meet-ai.exe`), and that `taskkill` without `/F`
   ends the relaunched tray app or not (only reported, not graded).
3. **Fixture accuracy on a Mac with Apple's en-US model.**
   `cargo test -p stt --test accuracy --test silence --test live -- --nocapture`
   Expect: no SKIPPED lines and every test passing, which shows the committed
   WAVs still meet the WER and timestamp bounds. Here the sidecar is built
   and the tests passed (Apple engine present); a runner without the model
   prints SKIPPED instead.
4. **The fixtures are byte-stable.** `git status` after `just fixtures` on
   another Mac will usually show the speech WAVs changed (`say` voices differ
   across macOS versions). That is expected; commit them only on purpose.

## Decisions

- Q1/A1: the two speech tracks are 1.92 MB each on disk (60 s of 16 kHz
  mono s16le), over the "under 1 MB" default; git stores them compressed at
  about 300 KB each, since 80% of each track is digital silence. Committed as
  they are (answer: a), rather than shortening the meeting (which would
  change `reference.json` and every timestamp test) or switching to FLAC.
  The two quiet fixtures are 0.96 MB each.

## Not done

- No `ffmpeg` in tests: the silence and room-tone fixtures are committed with
  the rest rather than made at test time, so no CI OS needs ffmpeg.
- `scripts/signing/verify-tur10.sh` stays macOS-only: it checks TCC and code
  signing, which have no Windows or Linux counterpart.
