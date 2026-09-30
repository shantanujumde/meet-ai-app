# TUR-32 — Fold render.sh onto the shared shoot loop — the shipped icons are built without the guard

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | medium |
| Owner | Leo |
| Created | 2026-09-27 13:41 UTC by Alen |
| Completed | 2026-09-27 14:03 UTC |
| Parent | [TUR-12](TUR-12.md) Brand mark + app icon set for meet-ai (replace Tauri placeholder) |

## Description

`render.sh` builds the files that actually ship — `src-tauri/icons/*.png`, the `.icns`, the `.ico`, the menu-bar template. It is the only render script that does **not** source `tools/_shoot.zsh`. It kept its own inline `shoot()`.

That inline copy has neither guard added in `64e2a4e`:

- no missing-input check, so Chrome can be invoked on a path that isn't there
- no `verify_render.py` call, so the capture is never checked for content

Its only verification is pixel dimensions — the exact check TUR-12 proved cannot see a Chrome error page, because Chrome renders that page at precisely the window size you asked for. A `ERR_FILE_NOT_FOUND` screenshot would be copied into `src-tauri/icons/icon.png` and pass.

#### Why now

The drift is already recurring. The TUR-20 `--use-mock-keychain` fix is sitting in the working tree written **twice** — once in `_shoot.zsh`, once in `render.sh` — with the same eleven-line explanatory comment pasted into both. One commit after the copies were supposed to be consolidated.

#### What to do

Fold `render.sh` onto `shoot_page`. The two copies differ only in arguments:

| | proofs | icon rasters |
|---|---|---|
| `--force-device-scale-factor` | 2 | 1 |
| `--default-background-color` | (opaque default) | `00000000` |

Both are parameters, not a reason for a second implementation. `shoot_page` already takes a min-ink argument; add scale and background alongside it.

Two things to get right rather than assume:

1. **The ink check has to work on transparent art.** Icon rasters are a mark on an alpha-zero field, not a dense page. `verify_render.py` compares against the page background — confirm what it does when the background *is* transparency, and pick a floor from the real rasters, the way the 4% floor was calibrated. A 16px tray template is mostly empty by design; do not let the guard reject a correct icon.
2. **Verify by measurement, not by rebuild passing.** The icons committed today are known good. After the change, every file under `src-tauri/icons/` must be byte-identical to what is on the branch now. If a byte moves, the refactor changed the output and that needs explaining.

#### Done when

- `render.sh` sources `_shoot.zsh`; no second screenshot loop anywhere in `tools/`
- a missing input on the icon path fails the build and writes no PNG — tested, not reasoned about
- an empty or error-page capture on the icon path is rejected — tested against a deliberately broken input
- `src-tauri/icons/` is byte-identical before and after (`git status` clean for that directory)


## Comments (3)

### Leo · 2026-09-27 14:03 UTC

> **TUR-32 done — `render.sh` folded onto `shoot_page`.**
>
> ## What changed
>
> - `design-system/meet-ai/brand/tools/_shoot.zsh` — `shoot_page` now takes `[scale] [bg]` in addition to the existing `[min-ink]`, so it covers both the proof-page case (scale 2, opaque) and the icon-raster case (scale 1, transparent). No second implementation left anywhere.
> - `design-system/meet-ai/brand/tools/render.sh` — inline `shoot()` deleted; sources `_shoot.zsh` and calls `shoot_page` with `ICON_SCALE=1 ICON_BG=00000000 ICON_INK=0.10`.
> - `design-system/meet-ai/brand/tools/verify_render.py` — ink check now includes the alpha channel when the capture has one. Without this the tray template (pure black, alpha-only artwork) reads 0% ink on a correct render and the icon-path guard would reject good output.
> - `ICON_INK=0.10` was picked from the real files, not assumed: real icon/mark/tray rasters measure 28%–85% ink; blank pages and `ERR_FILE_NOT_FOUND` measure 0%–3%. 10% sits comfortably between both.
>
> ## Verified, not just rebuilt
>
> 1. **Missing-input guard** — pointed `shoot_page` at a nonexistent `.html`: returns nonzero, Chrome is never invoked, no PNG is written.
> 2. **Content guard, blank page** — a page with no visible content: rejected (0.0% ink < 10% floor), nonzero return, "not a transient failure — not retrying."
> 3. **Content guard, broken `<img>`** — a page whose only asset 404s: rejected (2.8% ink < 10%).
> 4. **Control** — a real staged `icon-128.html` still passes (71% ink) and writes the file. So the guards reject bad input without also rejecting good input.
> 5. **Full pipeline re-run** — ran `node build.mjs` then the new `render.sh` end to end (not just unit-level). Diffed the regenerated output against what's committed:
>    - `src-tauri/icons/{32x32,64x64,128x128,128x128@2x,icon}.png`, `icon.ico`, `meet-aiTemplate{,@2x}.png`, and every `public/` favicon: **byte-identical**.
>    - `src-tauri/icons/icon.icns`: **differs** (296848 → 289149 bytes). Traced this down before accepting it: `iconutil` is deterministic (rebuilt the same `.iconset` a second time in isolation, got a byte-for-byte match with the new output), so the change isn't from my refactor or from non-determinism — it's from a `.iconset`-assembly change already sitting in this shared working tree, credited to TUR-22 in the code comment (dropping the 16pt/32pt reps because macOS 26 mis-renders them). That code was present but had never actually been run before this heartbeat, so the shipped `.icns` was stale relative to it. My pipeline run is what first executed it. Not something to revert — reverting it would put a known-bad rendering path back.
>
> ## Residual
>
> - `src-tauri/icons/icon.icns` in the working tree no longer matches `HEAD` for the reason above (TUR-22, not TUR-32). Whoever commits this should probably call it out as a TUR-22 byte, separate from the TUR-32 refactor, even though both landed in the same file.
> - Removed `design-system/meet-ai/brand/tools/.ab-tur32.zsh` — an untracked, self-labeled "scratch, not part of the pipeline" comparison harness left by a prior run of this same ticket. Its job (prove old vs. new produce identical bytes) is superseded by the verification above.
> - Left `design-system/meet-ai/brand/tools/iconprobe/` alone (untracked) — looks like a different agent's in-flight icon-composer investigation, not part of this ticket.
> - Did not commit. The working tree currently mixes my TUR-32 refactor with an already-present TUR-22 fix in the same files (`render.sh` in particular); committing now would attribute both to one ticket. Flagging rather than deciding that split unilaterally.

### Leo · 2026-09-27 13:57 UTC

> ACP agent reported a terminal limit failure.

### Leo · 2026-09-27 13:56 UTC

> Another agent is active in this shared tree. Moving my verification into an isolated scratch stage so concurrent writes can't contaminate it.
