# TUR-11 — redo assets with the new logo and assets designer

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Alen |
| Created | 2026-09-27 07:46 UTC by You (board) |
| Completed | 2026-09-27 14:09 UTC |

## Sub-tasks

- [TUR-12](TUR-12.md) **done** — Brand mark + app icon set for meet-ai (replace Tauri placeholder)

## Description

redo the assets with the new hire [@Leo](agent://a862228e-a7cb-40c1-ac13-e9e44cbac431?i=gem)

## Commits that mention this task

- `5f2d4d1` 2026-09-27 — TUR-11: land the generator that produces the fixed icon, not just the icon

## Comments (3)

### Alen · 2026-09-27 14:09 UTC

> TUR-22 closed, so the icon is now correct on screen and this is done.
>
> **The defect is gone.** `src-tauri/icons/icon.icns` ships six reps — 128/256/512 with their @2x — and no 16pt or 32pt entry. That is the whole fix. On macOS 26, a legacy `.icns` gets the old compositor whenever a rep exists at the requested point size, which pasted our already-rounded tile onto the system's own light plate: a square inside a square, illegible at 16px and double-framed at 32px. With no small reps, macOS synthesises the small end from the 128pt art and applies its own squircle, which fills the container correctly. Leo measured this rep-by-rep and confirmed it on two independently signed bundles. No artwork changed.
>
> **One gap I found and closed this heartbeat.** [TUR-22](TUR-22.md) committed the repaired `.icns`, but the script that *builds* it was left uncommitted, and [TUR-32](TUR-32.md) closed without picking it up. The tracked `render.sh` still copied `icon_16x16`/`16@2x`/`32x32`/`32@2x` into the iconset — so a single `./render.sh` would have quietly regenerated the exact broken icon, and the only copy of the fix was sitting in a working tree that [TUR-16](TUR-16.md) already showed us can lose files. Committed as `5f2d4d1`, scoped to `design-system/meet-ai/brand/`: the corrected `render.sh` with the measurements written above the iconset block, the shared `_shoot.zsh` loop and `verify_render.py`, the six `leo-tur22-{A..F}` proofs, and the `iconprobe/` harness (source only) that read icons back out of signed bundles.
>
> **What TUR-11 actually asked for, verified in the tree:**
> - `src-tauri/icons/` — every filename Tauri expects, a real 6-rep `.icns` and a real 7-entry `.ico` (16→256, 32-bit). No trace of the stock Tauri circle.
> - `design-system/meet-ai/brand/` — logomark, wordmark, horizontal lockups in ink/chalk/mono, favicon, menu-bar template, app-icon masters, `brand-tokens.css`, usage rules, concepts and 17 proofs.
> - `index.html` — SVG favicon with 32px/16px PNG fallbacks and an apple-touch icon; all four files present in `public/`.
> - Aria's rule held: the brand colour stays on the icon and marketing surfaces, the interface is still macOS system colours.
>
> **Two things stay open, neither gating this.** [TUR-37](TUR-37.md) — the macOS 14.4 floor in SPEC L2 is untested, because the fix means older releases downscale 16/32px from the 128pt art and this machine is Darwin 27 only; blocked on you supplying a 14.x/15.x machine or amending the floor. [TUR-35](TUR-35.md) — an Icon Composer `.icon` asset is the proper long-term fix for both OS versions at once, but it needs Xcode 26 and only Command Line Tools are installed; that one needs a call from you.
>
> **Separately, worth your attention.** Leo reported that two runs of the same agent were assigned and executing TUR-22 at the same time, both editing the same generated-asset pipeline. He caught it and stood his own edits down, but it could have shipped a half-merged icon build. That reads like a scheduler bug rather than anything either run did wrong, and it is the second time this pipeline has been hit by a write collision.

### Alen · 2026-09-27 13:43 UTC

> TUR-12 is done and the bulk of this landed. Holding TUR-11 open on one real defect rather than closing it.
>
> **What shipped.** `src-tauri/icons/` no longer holds the stock Tauri circle — it has the meet-ai set at every filename Tauri expects, plus a real `.icns` (10 reps) and `.ico`. `design-system/meet-ai/brand/` now has the full system: logomark, wordmark, horizontal lockups in ink/chalk/mono, favicon, a menu-bar template, brand tokens, usage rules, concepts and proofs. `index.html` is wired to an SVG favicon with 32px/16px PNG fallbacks and an apple-touch icon.
>
> **Why this isn't done.** [TUR-14](TUR-14.md) built and signed a real `meet-ai.app` and looked at it in the Dock. The art files pass every check — the shipped 16px rep on its own is clean — but **macOS 26 doesn't draw it that way.** IconServices re-renders legacy `.icns` app icons into the system's own icon container, and our art is already a rounded-square tile with its own padding, so it gets tiled inside a tile: illegible at 16px, double-framed at 32px. That's [TUR-22](TUR-22.md), open with Leo now. Calling the asset redo finished while the icon is unreadable in the menu bar would be reporting done on something you can see is wrong.
>
> **Two adjacent tracks that do not gate this one.** [TUR-23](TUR-23.md) is wiring a tray icon into app code that doesn't exist yet (the template PNGs are already in `src-tauri/icons/`), and [TUR-32](TUR-32.md) hardens the render script so shipped icons are built behind the same two guards as the proofs. Both are their own work, not the identity.
>
> **No decision needed from you.** Leo owns the fix; this reopens the moment TUR-22 closes.

### Alen · 2026-09-27 07:48 UTC

> Handed to Leo as **TUR-12 — Brand mark + app icon set for meet-ai**.
>
> **What I found.** Every visual asset in the repo is still a placeholder: `src-tauri/icons/` holds the stock Tauri blue circle at all seven sizes, `index.html` has no favicon, and there is no logo, wordmark, or brand file anywhere. So this is a from-scratch identity, not a refresh.
>
> **What I told Leo to respect.** Aria's design system in `design-system/meet-ai/` already decided that the interface uses macOS system colors and has no brand blue, deliberately. Leo's brand color lives on the icon and marketing surfaces only — he is not to recolor the UI. The icon has to sit naturally beside native macOS 26 app icons.
>
> **Deliverables in the brief:** direction options with 512px and 16px previews, the chosen logo system as SVG (symbol, symbol + wordmark, one-color), a full replacement icon set at the exact filenames Tauri expects including real `.icns` and `.ico`, a favicon wired into `index.html`, and brand source files with usage rules under `design-system/meet-ai/brand/`.
>
> **Your one decision.** Leo will come back with two or three mark directions and a recommendation. Picking the direction is yours — he'll set TUR-12 to review when the options are up. Everything after that he carries to done on his own.
