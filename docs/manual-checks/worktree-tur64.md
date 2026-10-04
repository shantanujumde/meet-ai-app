# Manual checks: TUR-64 (drift-check on Windows and Linux, `--audio` mode)

These need a real Windows 10/11 PC with speakers (not headphones), a
microphone and a real 30-minute call, so none were run here.

What is covered without a device:

- `crates/audio/src/segments/drift_audio/tests.rs` (every `cargo test -p audio`
  on macOS, Windows and Linux): GCC-PHAT finds known positive and negative
  lags in synthetic noise; a constant 20 ms lag reads as offset with 0 drift; a
  synthetic 100 ppm drift reads as about 100 ppm and 6 ms/min; silence and
  unrelated tracks (the headphones case) are "not measurable"; a 30 s stretch
  of unrelated audio drops the lock once and it is acquired again.
- `drift-check` is a bin of `crates/audio`, so the Windows (`just
  check-windows`) and Linux (`cargo build --workspace --all-targets`) CI jobs
  build it. `cargo check --target x86_64-pc-windows-msvc -p audio --bins`
  passed on macOS.
- `drift-check --audio <dir>` was run on macOS against a generated pair of
  16 kHz WAVs with a 320-sample lag: offset 20.0 ms, 0.00 ppm, PASS, exit 0.
  A missing folder gives exit 2.

## 1. 30-minute Windows recording, anchor drift (Wave H)

- Run: record a 30-minute real call on Windows with speakers (no headphones),
  then `cargo run -p audio --bin drift-check -- <meeting audio dir>`.
- Expected: `PASS`, worst under 200 ms, exit 0.
- Why skipped: needs a real Windows PC, devices and a call.

## 2. Same recording, audio drift (`--audio`)

- Run: `cargo run --release -p audio --bin drift-check -- --audio <same dir>`.
- Expected: some locks and trusted points, `PASS`, exit 0; ppm and ms/min
  printed. Measure it: the anchor and audio numbers should roughly agree, but
  the real Windows device drift is unknown until measured.
- Record both numbers (ppm, ms/min, worst ms) in `docs/findings.md` for
  Windows. There is no per-OS drift section there yet, so none was stubbed.
- Why skipped: needs the recording from check 1.

## 3. Linux

- Same as checks 1 and 2 on Linux, under the Linux half ticket (blocked by
  TUR-38).
