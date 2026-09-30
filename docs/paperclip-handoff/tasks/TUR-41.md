# TUR-41 — Approximate the pre-macOS-26 downscale at 16px and 32px — no 14.x host needed

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Leo |
| Created | 2026-09-27 14:09 UTC by Alen |
| Completed | 2026-09-27 14:17 UTC |
| Parent | [TUR-37](TUR-37.md) Check the icon on the macOS 14.4 floor — the TUR-22 fix trades the small end away |

## Description

#### Why this exists

[TUR-37](TUR-37.md) is parked on the board because there is no macOS 14.x or 15.x machine in this workspace. That hardware gap is real — I checked: this host is macOS 27.0 (build 26A428) and there is no VM tool installed. But the board is currently being asked to choose between buying a machine and cutting the supported OS range **with no data at all**, and a good chunk of that data can be produced here, today, on macOS 26.

This ticket produces that data. It does not replace the real check — it makes the board's decision an informed one, and it may make the real check unnecessary.

#### The question TUR-37 is actually worried about

On macOS 14.4/15.x the OS does **not** re-render a legacy `.icns`. It uses the reps it is given. After commit `57b655e` we give it nothing below 128pt, so it will downscale from the 128pt art. The hypothesis is "correctly composed but soft." The fear is "illegible smudge" — which is the same user-visible failure TUR-22 was opened for.

That is a question about **what the downscaled bitmap looks like**, and that is reproducible here.

#### What to do

1. Produce the downscale the way older macOS would: from the shipped 128pt rep (`ic07`) and the 128@2x rep (`ic13`, 256px) in `src-tauri/icons/icon.icns`, render down to the four cases that matter — 16px @1x, 32px @1x, 16pt @2x (32px), 32pt @2x (64px). Use Core Graphics / Core Image high-quality interpolation, not a nearest-neighbour or naive box filter; that is the closest honest stand-in for what IconServices does.
2. Put them on one sheet at nearest-neighbour magnification, next to two references: the **old hand-tuned 16px rep** (recoverable from `icon.icns` at HEAD~1, hash `989a1fe6…`, which still had all 10 reps) and the **macOS 26 render** already captured in `design-system/meet-ai/brand/proofs/leo-tur22-E-fixed-ladder-16-to-128.png`.
3. Say plainly which bucket the result falls in: **soft but readable** (the mark still reads as the mark) or **illegible** (brackets collapse, same failure as TUR-22).

#### Be honest about what this is not

A simulated downscale is a **proxy**, not the real thing. macOS 14's exact filter, its rep-selection rule, and any sharpening it applies are not guaranteed to match. Say so in the writeup. If the proxy comes out clearly readable, that is strong evidence the risk is low but it is not proof; if it comes out illegible, that is close to proof that the risk is real and the board should stop weighing and act.

#### Done means

A comment on this ticket with the sheet attached, the verdict in one sentence, and an explicit note on how far the proxy can be trusted. Then comment the same verdict on [TUR-37](TUR-37.md) so the board is choosing with it in hand.

#### Not in scope

- Changing any artwork or any `.icns`. This is measurement only.
- Whether to ship an Icon Composer `.icon` — that is [TUR-35](TUR-35.md).
- The real 14.x check itself. That still needs a machine and stays on [TUR-37](TUR-37.md).

## Commits that mention this task

- `b8218f8` 2026-09-27 — TUR-41: proxy the pre-macOS-26 legacy .icns downscale at 16/32/64px

## Work products

- artifact: leo-tur41-downscale-proxy-16-32-64.png

## Comments (1)

### Leo · 2026-09-27 14:17 UTC

> Ran the proxy asked for in this ticket. Sheet attached: `leo-tur22`-style comparison of the OLD hand-tuned 16/32/64px reps, this ticket's Core Graphics downscale proxy, and the current macOS 26 render, all nearest-neighbour magnified to the same size so softness is comparable.
>
> **Method:** extracted `ic07` (128pt, 128px) and `ic13` (128@2x, 256px) from the current `src-tauri/icons/icon.icns`, then downscaled each with a Core Graphics context set to `interpolationQuality = .high` (a Swift script, source included in the sheet's captions) to the four target sizes: 16px@1x, 32px@1x, 32px@2x (16pt on Retina), 64px@2x (32pt on Retina). The "OLD hand-tuned rep" column comes from `icon.icns` as it was one commit before TUR-22's fix (blob `4aebcfab5a`, the last version with all 10 reps, including the hand-tuned 16/32px art). The "macOS 26 current" column is a reused crop from `leo-tur22-E-fixed-ladder-16-to-128.png` (only the 16 and 32pt tiers line up pixel-for-pixel with this test; the ladder's other tiers are 41/44pt, not exact matches for the 2x cases, so those cells are marked "no matching capture" rather than faked).
>
> **Verdict: soft but readable at every size tested (16, 32, 64px) — no collapse, no illegible smudge.**
>
> At 16px the proxy is visibly softer and greyer than the old hand-hinted rep (the old one was pixel-snapped by a person; the proxy is a pure algorithmic downscale, so it has more anti-aliased grey transitional pixels and a less crisp tile edge). But the bracket-and-dot shape is still clearly identifiable at a glance against the dark tile — the brackets don't merge or disappear, and the orange centre is still a distinct blob, not a wash. At 32px and 64px the proxy is close to indistinguishable from the old hand-tuned rep. Full crops (20x nearest-neighbour, on a dark background) are in the sheet.
>
> **How far this can be trusted:** this is a proxy, not the real check. It tells you what a generic high-quality Lanczos/bicubic-class resample of the 128pt art looks like — it does NOT reproduce macOS 14.x/15.x's exact filter, its rep-selection rule, or any OS-side sharpening, and it cannot reproduce the separate nested-tile defect from TUR-22/the macOS 26 render, because that comes from OS icon *composition* (Icon Composer/newer rendering), not from resampling a flat bitmap. Given the result, I'd read this as evidence the "illegible smudge" fear is unlikely, but it is not proof — TUR-37 still needs a real 14.x/15.x host to close out, if the board wants certainty rather than strong evidence.
>
> Also confirmed as a sanity check: downscaling the 32px target from `ic07` (128px) directly vs. from `ic13` (256px) gives near-identical results (max channel diff 18/255 on a handful of edge pixels) — so the "@1x vs @2x source" choice doesn't change the story.
>
> No artwork or `.icns` changed — measurement only, as scoped.
