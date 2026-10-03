# Manual checks: TUR-41 (THIRD_PARTY_NOTICES.md and rule R9)

What did run: `scripts/quality-rules-selftest.sh` under macOS `/bin/bash` 3.2
(20 of 20 cases pass, including "an `Adapted from` line with no notice entry
fails" and "one with an entry passes"); `shellcheck` on both scripts;
`actionlint` on `.github/workflows/check.yml`; and the quality gate on the
branch's files (result in the PR body). Nothing here needs hardware, a
permission or an account. What could not be checked from the worktree is
below.

## 1. The self-test runs in CI on ubuntu

The new step `Quality rules self-test` is in the `rust-portable` job, which
runs on ubuntu with GNU awk and bash 5, not the Mac's.

1. Open this PR's `rust fmt + windows seam (ubuntu)` job. Expected: the step
   prints 20 `ok` lines and `20/20 passed`, and the job is green.

## 2. R9 in the `rust` job's quality-rules step

1. On a later PR that copies code, look at the `Quality rules on changed
   files` step. Expected: no R9 line when the copied file's `Adapted from`
   line matches a `THIRD_PARTY_NOTICES.md` section; an `R9 ERROR` and a red
   job when it does not.

## 3. Things left for a person to decide

- `THIRD_PARTY_NOTICES.md` has no confirmed sources yet. AudioCap, sudara's
  tap gist and swift-scribe are listed under "To confirm": the docs name them
  as models, but no file says it was adapted from them, and the commit used is
  not recorded. Someone who wrote the Phase 0a tap
  (`spikes/phase0a-tcc/src/probe/main.swift`,
  `crates/audio/src/macos/tap.rs`) should say whether code was copied from
  AudioCap. If it was, move AudioCap into a full section with the BSD-2-Clause
  text and add the `Adapted from` line to those files.
- `.claude/skills/quality-gate/SKILL.md` has a table of R1 to R8. This run may
  not edit `.claude/**`, so R9 is not in it yet. Add a row:
  `| R9 | an "Adapted from" comment with no THIRD_PARTY_NOTICES.md section, a GPL/AGPL/LGPL or missing SPDX id, or no commit hash | add the project's section to THIRD_PARTY_NOTICES.md; for GPL code, write our own |`.
- `docs/quality-rules.md` still says "the repo rules R1–R8 below" in its
  first table. Left as is, so this branch only adds lines; it can say R1–R10
  once TUR-42 has landed too.
