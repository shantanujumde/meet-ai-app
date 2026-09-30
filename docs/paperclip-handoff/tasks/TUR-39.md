# TUR-39 — Watchdog review for TUR-15

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Alen |
| Created | 2026-09-27 14:06 UTC by — |
| Completed | 2026-09-27 14:08 UTC |
| Parent | [TUR-15](TUR-15.md) Phase 1b — live transcription session API (streaming seam for the Phase 2 pane) |

## Description

Task watchdog review issue.

Watched issue: TUR-15
Stopped fingerprint: task_watchdog_stop:7ad5926a83b3d801817956b06bc90487a383a96ef40883dd61dc9b758d646598

The watchdog agent should verify the stopped subtree and either confirm the disposition or restore a valid live path.

## Comments (2)

### Alen · 2026-09-27 14:08 UTC

> ## Verdict: the stopped subtree is genuinely finished. No recovery needed.
>
> Watched: **TUR-15** (`done`). Non-watchdog descendants: **TUR-31** (`done`). Nothing else hangs off it, so the whole subtree is two closed leaves.
>
> I checked the claims against the repo rather than the reports.
>
> ### TUR-15 — all five deliverables exist, and the consumer signed off
>
> | Deliverable in the ticket | Evidence |
> |---|---|
> | 1. The seam (`start_session` / `feed` / `finish`) | `crates/stt/src/session.rs` present |
> | 2. Volatile tail contract | same file; Nia read `session.rs` directly and confirmed `Dropped { speaker, seq }` matches what her pane needs |
> | 3. Meeting-global `seq` | `SeqCounter`, asserted unique across both tracks |
> | 4. Replay session | `crates/stt/src/replay.rs` + `just live-replay` |
> | 5. Live silence gate | `crates/stt/tests/live.rs` present |
>
> Claimed head commit `4c3f562` ("TUR-15: print what whisper streamed…") exists in branch history. Reported: 112 tests green, clippy clean.
>
> The strongest signal is not the author's own report — it is **Nia's comment at 14:06**, from the consuming side (TUR-6). She read the source, confirmed the contract works for her pane, and explicitly wrote "TUR-15 stays `done`". That is an independent party accepting the deliverable, which is what the success condition asked for.
>
> ### TUR-31 — the open decision was answered, and built
>
> The ticket asked for a choice between stdin and a growing file. The answer is **stdin**, with a measured reason the growing-file path is not merely worse but blocked (`AVAudioFile` fixes length at open; the SPEC §3.3 header rewrite lags the samples). Rune did not stop at a decision — commit `6954f15` ("feed meet-stt a live pipe…") is in branch history, and `sidecar/meet-stt/main.swift` really carries `--stdin` (arg parsing at :125, mutual-exclusion guard at :605, `ready` line documented at :32). Reported: `cargo test -p stt --test sidecar`, 11 passed.
>
> ### The unfinished part is not orphaned
>
> TUR-15 deliberately left the Apple streaming path unwired, and said so honestly in every update rather than claiming it. That work is **TUR-33** ("Wire AppleEngine onto the live session seam"), which is `in_progress` with a live run and already has the handoff on its thread: the room-tone volatile hallucination (`room-tone-30s.wav` → one phantom volatile, `"I"`), the new `ready` protocol line, and the warning not to hard-code the analyzer format. So the one finding worth losing did not get lost.
>
> ### Why I did not re-run the test suite
>
> The checkout is shared, and `crates/stt/src/apple.rs` has uncommitted work from the TUR-33 run happening right now. Building there would either fight that agent's build lock or fail on someone else's mid-flight edits — a red result that says nothing about TUR-15. The committed artifacts, the independent consumer sign-off, and the live successor issue are the stronger evidence here.
>
> ### One thing for the board, outside my reach
>
> TUR-33 is a **root issue**, not a child of TUR-15, so the remaining Phase 1b work sits outside this watched subtree and outside this watchdog's cover. That is fine today — it has an active run — but it means no watchdog is attached to the piece of Phase 1b that is still open. Worth a parent link if the board wants that safety net. I am leaving it alone: mutating outside the watched tree is not mine to do.
>
> **Disposition: confirmed. No leaf reopened, no follow-up created, no blocker.**

### Paperclip (automatic) · 2026-09-27 14:06 UTC

> Task watchdog started for stopped subtree.
> Watched issue: TUR-15
> Stopped fingerprint: `task_watchdog_stop:7ad5926a83b3d801817956b06bc90487a383a96ef40883dd61dc9b758d646598`
> Stopped leaves:
> - No leaf issues found.
