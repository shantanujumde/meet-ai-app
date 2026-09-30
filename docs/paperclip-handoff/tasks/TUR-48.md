# TUR-48 — create first release

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 15:47 UTC by You (board) |
| Completed | 2026-09-28 12:20 UTC |

## Description

_No description._

## Comments (13)

### Alen · 2026-09-28 12:20 UTC

> ## Correction — this is done, not blocked
>
> I moved this toward `blocked` an hour ago on the strength of the stall alone. That was wrong. Checked the repo instead of the board, and the work exists:
>
> - `v0.1.0` points at `3f6c4e4`, the merge commit for PR #1.
> - `v0.2.0` is also tagged, so the first release has already been superseded.
> - The health gate ran green on that commit: fmt, clippy, 231 Rust tests, biome, typecheck, 24 vitest.
>
> You merged and tagged it yourself on 27 Sep and then asked for status and next steps, which I answered the same afternoon. Nothing was left to do — the issue just never got closed, and the runs that fired afterwards were the shared-connection failures, not real work.
>
> Closing as done. `gh` here is still unauthenticated (401), so if you want a GitHub *Release* page rather than just the tag, that is a separate small task and needs a logged-in `gh`.

### Alen · 2026-09-28 12:13 UTC

> ## Moving this to blocked — it has been stuck with nobody able to move it
>
> This sat in review for **18 hours** with no path out. The board's own check said so: *"in review with an agent assignee but no participant, interaction, approval, user owner, wake, active run, or recovery issue owning the next action."* Re-pinging it, which I did this morning, could never have helped — there was nothing for an agent to do.
>
> ### What is actually blocking it
>
> The GitHub token on this machine is invalid, and the repo is private. Checked again just now:
>
> ```
> X Failed to log in to github.com account shantanujumde (default)
>   - The token in default is invalid.
> ```
>
> So I cannot see whether the `v0.1.0` tag was pushed, or whether a Release page exists. Everything that does not need GitHub is already done and verified locally:
>
> - `main` and `origin/main` in sync, PR #1 merged
> - tag `v0.1.0` exists locally on merge commit `3f6c4e4`
> - `CHANGELOG.md` and `SECURITY.md` on `main` with the 0.1.0 entry
> - signed bundle at `target/release/bundle/macos/meet-ai.app`, `codesign --verify --strict` passes
> - full health gate green: 231 Rust tests, 24 Vitest, clippy/fmt/biome/typecheck all clean
>
> ### What unblocks it
>
> Run this in a terminal, then say so here:
>
> ```
> gh auth login -h github.com
> ```
>
> Then I will confirm the tag is pushed, publish the Release page for v0.1.0, and close this out.

### Alen · 2026-09-28 10:40 UTC

> **Auto-recovery — re-ping 1 of 3**
>
> Run `d747381a-a7be-427b-a6a0-df1eba6351d9` stopped after 5s without starting a turn: `Run ended with failed (acpx_turn_failed) ACP agent reported a terminal access failure. ACP agent reported a terminal access failure.` This is the shared Claude connection refusing a session, not a failure in this issue's work, so the run is being started again.
>
> If this comment repeats without progress, the connection is still refusing. Next attempt in 30 minutes.

### Alen · 2026-09-27 18:05 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 18:04 UTC

> ACP agent reported a terminal access failure.

### Alen · 2026-09-27 16:49 UTC

> ## Health gate result — green, with one network caveat
>
> Ran every part of `just check` on `main` at `3f6c4e4`:
>
> | Step | Result |
> |---|---|
> | `cargo fmt --all --check` | clean |
> | `cargo clippy --workspace --all-targets -D warnings` | clean |
> | `cargo test --workspace` | **231 passed, 0 failed**, 2 ignored |
> | `pnpm biome check .` | 50 files, clean |
> | `pnpm typecheck` | clean |
> | `pnpm vitest run` | 24 passed |
> | `just check-windows` | **could not run** |
>
> The two ignored tests are the hardware ones (`mic_closed_loop`) that need someone to grant mic and system-audio permission — that is the Phase 0 gate, not a failure.
>
> `check-windows` failed downloading the `windows` crate from crates.io (repeated 503s and "transfer too slow"). That is this machine's network, not the code — nothing in the repo changed since it last passed. Re-run `just check-windows` when the network is healthy to confirm.
>
> So: **v0.1.0 is green on its own tests.**
>
> ## GitHub is still dark to me
>
> The `gh` token is still invalid and `git ls-remote` still says "Repository not found", so I cannot see the Release page you created. Local state matches what a finished release looks like: `main` = `origin/main`, tag `v0.1.0` on `3f6c4e4`, `CHANGELOG.md` and `SECURITY.md` in place, signed `.app` bundle passing `codesign --verify --strict`. Run `gh auth login -h github.com` whenever you want me to verify the published side.
>
> ## Next steps
>
> The board stops at Phase 2. Everything past it — store and search, the agent loop, calendar detection — has no task yet, which is why `crates/store`, `crates/calendar`, `crates/detect` and `crates/prompts` are still documentation-only stubs.
>
> The single most valuable thing is **not** more code: it is the 45-minute real call. TUR-4, TUR-5, TUR-6 and TUR-7 are all blocked behind it, and it only needs you and a Mac with permissions granted. One session unblocks four issues.
>
> I have put a card below with the five tasks I think should exist next. Pick the ones you want and I will create them.

### Alen · 2026-09-27 16:36 UTC

> ## Release status — verified locally
>
> - PR #1 is merged; `main` and `origin/main` are in sync (0 ahead, 0 behind).
> - Tag `v0.1.0` exists and points at the merge commit `3f6c4e4`.
> - `CHANGELOG.md` and `SECURITY.md` are on `main` with the 0.1.0 entry.
> - A signed app bundle exists: `target/release/bundle/macos/meet-ai.app`, identifier `pro.saleschat.meetai`, authority `meet-ai Local Signing`, `codesign --verify --strict` passes. No `.dmg` was built.
> - **I still cannot see GitHub.** The `gh` token is invalid and the repo is private, so I can't confirm the tag was pushed or that a Release page exists. Run `gh auth login -h github.com` and I can verify and finish it.
>
> ## What actually works in 0.1.0
>
> - **Audio capture** — mic and system audio, resampled to 16 kHz, crash-safe WAV writes, `segments.json` drift contract with 18 fixtures, `meet-rec` CLI. Verified against real hardware.
> - **Speech-to-text** — Apple `SpeechTranscriber` via the Swift sidecar plus a whisper.cpp fallback, live streaming seam, silence gate, resumable model downloader.
> - **App shell** — Tauri 2 + React 19: meetings list, review view, notes pane, onboarding with the permission-denied path, menu-bar tray, typed IPC.
> - **Permissions and signing** — positive-control tone check, inside-out bundle signing that fails loudly.
>
> ## What is not built
>
> `crates/store`, `crates/calendar`, `crates/detect` and `crates/prompts` are documentation-only stubs. That means SPEC §5 Phases 3 (store + FTS5 search), 4 (agent loop and tickets), 5 (calendar and meeting detection) and 6 (polish) have not started — roughly half the 10.5-week plan. No notarization, no Windows, no MCP server.
>
> ## The one gate that matters and is still open
>
> TUR-4, TUR-5, TUR-6 and TUR-7 are all `blocked`, and they are blocked on a person, not on code. SPEC §5's Phase 0 exit gate is a **45-minute real call**: both tracks intact, drift under 200 ms end to end, survives an AirPods switch mid-call, survives `kill -9`. TUR-4 also waits on someone granting microphone and system-audio permission on this Mac and running:
>
> ```
> cargo test -p audio --test mic_closed_loop -- --ignored --nocapture
> ```
>
> Until that run happens, 0.1.0 is "builds and passes its own tests", not "proven on a real meeting".
>
> ## Next steps, in order
>
> 1. **Re-auth GitHub**, push the tag, publish the Release — minutes, and it closes this issue.
> 2. **Run the 45-minute real-call gate.** Unblocks TUR-4, TUR-5, TUR-6, TUR-7 at once.
> 3. **Phase 3 — store + index**: markdown read/write, file watcher with self-write suppression, SQLite FTS5, search box. Exit gate: delete `index.db`, everything still works after a rescan.
> 4. **Phase 4 — agent loop**: the `[Wrap up]` / `[Start Work]` / `[Push ticket]` prompt buttons.
> 5. **Phase 5 — detection + calendar**: EventKit first, cloud calendars after.
>
> Smaller open items on the board: TUR-19 (stray windows opening), TUR-35 (ship an Icon Composer `.icon`? needs Xcode 26), TUR-16 (agents sharing one git checkout, uncommitted work being lost), and TUR-21 / TUR-28 / TUR-44 sitting in review.
>
> I am running `just check` now and will report the result.

### You (board) · 2026-09-27 16:33 UTC

> I did it myself, tell me the status of the app, what are the next steps

### Alen · 2026-09-27 16:30 UTC

> cancelled

### Alen · 2026-09-27 16:24 UTC

> Not stuck on the local work — stuck on one thing only: **GitHub access**.
>
> What is done:
> - CHANGELOG.md and SECURITY.md are written and committed (0.1.0 entry dated today).
> - Branch `chore/claude-setup-and-design-system` is 67 commits ahead of `main`, 0 behind → clean fast-forward merge.
> - Health gate `just check` is running now.
>
> What I cannot do:
> - `gh` token is invalid and `git ls-remote` returns "Repository not found" for `shantanujumde/meet-ai-app`. So I can merge and tag locally, but cannot push or publish a GitHub Release.
> - Fix: run `gh auth login -h github.com` on your machine, then tell me and I will push + publish.
>
> Proceeding with `v0.1.0` (matches package.json, Cargo.toml, tauri.conf.json and the CHANGELOG). SPEC §8.1 puts public release at v2, so 1.0.0 would overstate it. Say the word if you want `v1.0.0` instead and I will re-version before tagging.

### You (board) · 2026-09-27 16:23 UTC

> are you stuck

### Alen · 2026-09-27 16:13 UTC

> Started the release. Findings so far:
>
> - `chore/claude-setup-and-design-system` is 63 commits ahead of `main` and 0 behind, so the merge is a clean fast-forward.
> - Running `just check` (the repo's single health gate) before touching `main`.
> - **GitHub is not reachable**: the `gh` token is invalid and `git ls-remote` returns "Repository not found". I can merge and tag locally, but I cannot push or publish a GitHub Release until you run `gh auth login -h github.com`.
> - SPEC 8.1 asks for a `CHANGELOG.md` and `SECURITY.md` "from commit one" and neither exists. Adding both as part of this release.
> - Version is `0.1.0` in package.json, Cargo.toml and tauri.conf.json. Tagging `v0.1.0` unless you want 1.0.0 — will ask.
>
> Will report back with the gate result and the built bundle.

### You (board) · 2026-09-27 16:11 UTC

> merge branch to main and release v1
