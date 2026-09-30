# TUR-82 — option to change dir

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Nia |
| Created | 2026-09-28 06:01 UTC by You (board) |
| Completed | 2026-09-28 06:23 UTC |

## Description

![](/api/assets/15d9cfd4-7bc7-4edc-985a-367c1b3bc7d2/content)

## Commits that mention this task

- `249bc3c` 2026-09-28 — TUR-82: add an option to change the meetings folder

## Comments (2)

### Nia · 2026-09-28 06:23 UTC

> Done. Added a "Change…" option for the meetings folder, in both Settings and the onboarding folder step (they share one hook so the two screens can't disagree).
>
> **What happens to existing files (the part you specifically asked me to cover):** they always move. Picking a new folder shows a plain-language confirmation naming the old and new paths and saying nothing is deleted, then moves every meeting folder over — merging into the new folder rather than overwriting if it already has something in it, and refusing outright (with a clear message) if a name would collide. Nothing is ever silently left behind at the old location, so the meeting list never appears to lose history just because the folder changed. Refused with a clear message if a recording is in progress, since the recorder is mid-write to the old folder.
>
> **Where I put this in the code, and one deliberate scope note:** `SPEC.md`'s Phase 6 was slated to own `meetings_root`, folded into `config.jsonc`. I didn't wait for that — `config.jsonc` itself lives *inside* the meetings root, so the chosen root can't be recorded there without a bootstrapping problem. I gave it its own small pointer file in the OS's standard per-app support folder instead (just the path, nothing meeting-shaped), and updated `config.rs`'s doc comment so the next person working on Phase 6 isn't surprised `meetings_root` is already handled elsewhere. This is additive and narrowly scoped (just the folder itself, not the rest of Phase 6's config/JSON-schema work) — flagging it per my charter since it's technically ahead of the phase order, not because it conflicts with any locked L-decision.
>
> **Verified:**
> - `cargo test -p meet-ai --lib meetings::` — 14 tests pass, including 4 new ones covering the plain move, the merge-into-an-existing-folder case, and the name-collision refusal.
> - `cargo clippy -p meet-ai --lib -- -D warnings` — clean.
> - `pnpm biome check` + `pnpm tsc --noEmit` + `pnpm vitest run src/App.test.tsx` on the changed frontend files — all clean/passing.
> - Committed (249bc3c).
>
> **Not verified:** I did not launch the actual Tauri app and click through the picker/confirm/move flow in a real window — this environment has no display, so I can't confirm the native folder-picker dialog and confirmation dialog actually render and read well side by side with the design-system's dark theme. Worth a quick look from whoever can run `pnpm tauri dev` next, or a handoff to Tess for a visual pass.
>
> Left untouched on purpose: `crates/audio`, `permission.rs`, and `RecordControl.tsx` have unrelated uncommitted work in progress (looks like Rune's) — I worked around it rather than reverting or committing it.

### Alen · 2026-09-28 06:06 UTC

> Routed to you (Nia) from TUR-84 — settings and app shell are yours. The user wants to choose where recordings and data are stored instead of being stuck with the default location. Include what happens to existing files when the folder changes.
