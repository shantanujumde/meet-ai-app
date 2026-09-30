# TUR-86 — Verify the Icon Composer .icon on a real signed bundle

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **blocked** |
| Priority | low |
| Owner | Tess |
| Created | 2026-09-28 06:11 UTC by Alen |
| Parent | [TUR-35](TUR-35.md) Decide whether to ship an Icon Composer .icon (needs Xcode 26, not installed) |
| Blocked because | Waiting on TUR-85 (backlog) |

## Description

Blocked on Rune's bundle pipeline. Same harness as [TUR-14](TUR-14.md): `design-system/meet-ai/brand/tools/iconprobe/`.

Verify the Icon Composer `.icon` on a **real signed bundle**, not a staged render. The whole point of TUR-35 is that the `.icon` controls the icon container where a legacy `.icns` does not, and that only shows up in a real bundle the system has rendered.

#### What to measure

- Every rung of the ladder Leo measured in TUR-22 and TUR-14 — Dock and Finder, all sizes, including 16px and 32px.
- Compare against the **current `.icns` baseline**, which is correct on macOS 26 at every size Leo could measure. The `.icon` has to be at least as good; if it is only equal, say so.
- Confirm the `.icns` fallback still works — both formats ship, so check nothing regressed for the legacy path.
- `codesign --verify --deep --strict` on the bundle, since Rune's step re-signs after modifying it.

#### The question worth answering

TUR-35's benefit list did not survive fact-checking — see plan section 1 on TUR-35. If your rungs come back **indistinguishable** from today's `.icns`, that is the honest result and worth recording, not a failure. It tells us the ticket bought platform-alignment and nothing visible.

#### Done when

- Measurements posted, with the TUR-22 baseline alongside for comparison.
- A clear verdict: better / same / worse, at each size.

## Comments (4)

### Tess · 2026-09-28 12:31 UTC

> Acknowledged — expectations noted for when a real bundle lands:
>
> 1. **Success criterion is parity, not improvement.** Every rung matching the TUR-22 baseline (Dock + Finder, all sizes incl. 16/32px) is the pass condition. I'll only flag a rung if it's *worse* than baseline, and call out clearly if everything is indistinguishable rather than writing that up as a null result.
> 2. **`codesign --verify --deep --strict` after the `Assets.car` copy + `Info.plist` merge is the real test**, since that's the step that invalidates Tauri's own signature and re-signs. This gets equal billing with the visual rungs in my report, not an afterthought.
> 3. Will also confirm `Assets.car` is actually consumed (not shadowed by the `.icns` still in the bundle) and that the legacy `.icns` fallback path still works.
>
> Re-checked the dependency chain before doing anything else: TUR-85 is still `backlog` — it's now itself blocked on TUR-87 (Leo authoring `meet-ai.icon` in Icon Composer, also `backlog`) plus Xcode 26 actually being installed in the workspace. So there's still no `actool`-compiled `Assets.car`, no re-signed bundle, nothing for `codesign` to check yet. A staged render wouldn't answer the container-control question this ticket exists to answer, so I'm not fabricating rungs against a mock.
>
> TUR-86 stays `blocked`. Owner/action: Rune on TUR-85, which itself needs TUR-87 (Leo) to land first. I'll pick this back up the moment a signed bundle with the `.icon` compiled in exists.

### Alen · 2026-09-28 12:30 UTC

> **One expectation to set before you run the rungs.**
>
> I settled the open question on this chain: an Icon Composer `.icon` is a **single 1024 composition** that the system renders every size from — there's no per-size artwork override in the format. So the `.icon` is not expected to look *better* than today's `.icns` at 16px or 32px on macOS 26. It's expected to look the same.
>
> That means: **rungs indistinguishable from the TUR-22 baseline is the success case, not a null result.** The point of the change is that the icon container is ours rather than one the system picks for us — a correctness-and-durability property, not a visible one. What would be a real finding is a *regression* at any rung, or the signature failing after Rune's post-bundle re-sign.
>
> So the two things worth your attention:
> 1. Every rung matches the TUR-22 baseline or better — flag any that got worse.
> 2. `codesign --verify --deep --strict` passes on the bundle *after* the `Assets.car` copy and `Info.plist` merge. That step invalidates Tauri's signature and re-signs; it's the newest, least-tested surface in the whole chain.
>
> Also worth confirming once it's live: that `Assets.car` is actually being used, not silently ignored in favour of the `.icns` still in the bundle. Both ship.
>
> Plan document with the full recipe is on [TUR-35](TUR-35.md), revision 2. Still blocked behind [TUR-85](TUR-85.md).

### Tess · 2026-09-28 12:27 UTC

> Checked in on the "continue" ping: TUR-85 (Rune's actool/CFBundleIconName/re-sign pipeline) is still `backlog` with no comments or assignee activity, so there is no signed bundle with the `.icon` compiled in yet. Nothing to measure on real hardware without that artifact — a staged render would not answer the question this ticket exists to answer (whether the `.icon` controls the icon container in a way `.icns` cannot).
>
> No action taken on the deliverable itself; re-checking TUR-85 status is the only thing that changes this. Leaving TUR-86 blocked on TUR-85, owner @Rune. Will pick this up as soon as TUR-85 ships a re-signed bundle.

### You (board) · 2026-09-28 12:27 UTC

> continue
