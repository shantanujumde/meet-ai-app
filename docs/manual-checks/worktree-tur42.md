# Manual checks: TUR-42 (OS code into platform modules, rule R10, stub-audio)

What ran in the worktree: `just check` (fmt, clippy, the whole workspace's
tests, biome, typecheck, vitest, plus `just check-windows`), the `-p audio`
tests a second time with `--features stub-audio`, `cargo clippy -p audio
--all-targets --features stub-audio -- -D warnings`, `cargo check --target
x86_64-pc-windows-msvc` on `audio` (with and without `stub-audio`), `stt`,
`meeting-format` and `agent` (lib and tests), `scripts/quality-rules-selftest.sh`
under macOS `/bin/bash` 3.2 (45 of 45 cases, 25 of them R10), and the quality
gate (result in the PR body). With R10 taken out of `RULES`, 12 of the R10
cases fail, so the cases do test the rule. A scan of every tracked `.rs` file
on this branch, every line counted, finds only the three test gates in
section 4. That held on this branch only: TUR-45, merged alongside, added four
OS cfgs to `crates/store` (retention's Windows lock check and its tests), which
R10 never saw because it checks added lines. TUR-89 moved them into
`crates/store/src/platform/`, and CI now runs R10 over the whole tree.

None of that records audio, so the app itself was not run. Nothing on macOS
should behave differently; section 1 is how to confirm it.

## 1. macOS smoke test: recording behaves as before

The capture, permission and device-watch code now goes through
`crates/audio/src/platform/`, so check the paths it touches. Needs a signed
build (`just bundle-signed`), a mic, speakers and the TCC grants.

1. Launch the signed app. Expected: onboarding or the meeting list, as before;
   no new permission prompt if mic and system audio were already granted.
2. Start a recording, play a minute of speech through the speakers (a YouTube
   talk) while saying a few sentences into the mic, then stop. Expected: the
   live transcript shows both "You" and "Others" lines while it records;
   the meeting folder has `mic.wav`, `system.wav` and `segments.json`, and
   `system.wav` is not silent.
3. Halfway through a second short recording, connect or disconnect AirPods.
   Expected: `segments.json` gets a second segment with reason
   `default-output-device-changed` (or `...-input-...`), as before. This is
   the device watch that now goes through `platform::default_output_device`.
4. When the recording ends, let the notes run (Claude Code or Codex, as set up).
   Expected: notes appear in the meeting, as before. The agent CLI is now put
   in its own process group by `crates/agent/src/platform/unix.rs`; press
   Cancel on a second run and check with `ps` that no `claude`/`codex`
   process is left over.
5. Settings, permission check. Expected: the mic and system-audio results are
   the same as before (a chime plays once for the system-audio check).

Not run here: it needs the running app, real devices, TCC grants and a
signed-in agent CLI.

## 2. The R10 self-test in CI (ubuntu, mawk)

The R10 cases are in `scripts/quality-rules-selftest.sh`, which the
`rust-portable` job already runs on ubuntu. That runner has mawk, not macOS
awk; the awk was kept to POSIX features for it, but only macOS awk ran here.

1. Open this PR's `rust fmt + windows seam (ubuntu)` job. Expected: the
   self-test step prints `ok` for every `R10:` case and `45/45 passed`, and
   the `Windows seam check` step (`just check-windows`) is green.

## 3. Linux

`crates/*/src/platform/linux.rs` was never compiled here: only the Windows
target may be installed on this Mac. They mirror the Windows files (audio and
stt) or re-export the shared `unix.rs` (agent, meeting-format). TUR-36's Linux
CI job is the first build of them.

## 4. Left for TUR-54

These unix-only test modules keep their `cfg(unix)` (decision A1: they run the
`/bin/sh` fake harness, and TUR-54 makes them portable). R10 only flags added
lines, so they pass until someone edits them.

- `crates/agent/src/process.rs:504` `#[cfg(unix)] mod unix` (in its tests)
- `crates/agent/src/detect.rs:401` `#[cfg(test)] #[cfg(unix)] mod tests`
- `crates/agent/src/mcp/tests.rs:190` `#[cfg(unix)] mod fake_cli`

## 5. Notes for later tickets

- `crates/audio/examples/switch_output.rs` is a macOS-only example (Core
  Audio calls, no `cfg`), so `cargo check --all-targets` for Windows fails on
  it. `just check-windows` does not build examples, so it is green. TUR-36
  needs to deal with it when CI builds all targets on Windows (for example
  `required-features`, or moving it under `src/macos/`).
- `crates/store` had no OS code on this branch (the ticket's grep hit is a doc
  comment), so it got no `platform` module here. TUR-45 added some in
  parallel; TUR-89 gave `store` its `platform` module.
- `src-tauri/src/lib.rs` and `notify.rs` keep Tauri's
  `cfg(not(any(target_os = "android", target_os = "ios")))` desktop gates.
  R10 exempts conditions whose only OS names are android/ios (decision A2).
- `.claude/skills/quality-gate/SKILL.md` has a table of the rules. This run
  may not edit `.claude/**`, so R10 is not in it yet. Add a row:
  `| R10 | a cfg on target_os / unix / windows / target_family outside src/platform/, macos/, windows/, linux/, eventkit.rs, build.rs or crates/*/tests/ | move the OS code into the crate's src/platform/ and call platform::... |`.
- `docs/quality-rules.md` still says "the repo rules R1–R8 below" in its
  first table. Left as is, so this branch only adds lines.
- `stub-audio` sources never produce a sample, so a `RecordingSession` over
  them fails at start ("no audio arrived"), which is what a stub with no
  samples means. Tests that need frames keep using their own stub sources, like
  `session/tests.rs` does.
