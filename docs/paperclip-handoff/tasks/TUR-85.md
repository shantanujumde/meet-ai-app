# TUR-85 — Compile the .icon into the Tauri bundle — actool, CFBundleIconName, re-sign

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **backlog** |
| Priority | low |
| Owner | Rune |
| Created | 2026-09-28 06:11 UTC by Alen |
| Parent | [TUR-35](TUR-35.md) Decide whether to ship an Icon Composer .icon (needs Xcode 26, not installed) |

## Description

Blocked on Leo's `.icon` source landing. Full recipe: the **Icon Composer .icon — implementation plan** document on [TUR-35](TUR-35.md), section 2.

This is the piece TUR-35 originally missed. Tauri does not compile asset catalogs — `src-tauri/tauri.conf.json:39` lists icons as a flat array ending in `icons/icon.icns`, the bundler copies those files in, and it **signs during bundling**. So a `.icon` is not a drop-in swap.

#### The work

1. **Compile.** `actool` turns the `.icon` into a binary `Assets.car`. Exact invocation is in plan section 2.1 — note `--app-icon Icon` must match the `.icon` filename stem, and `--minimum-deployment-target 26.0` matches `tauri.conf.json:41`.
2. **Info.plist.** Add `CFBundleIconName`. Keep the existing `.icns` keys as well; both ship.
3. **Post-bundle + re-sign.** After `tauri build`: copy `Assets.car` into `<App>.app/Contents/Resources/`, merge the plist key, then **re-sign** — steps 1 and 2 invalidate the signature Tauri just applied. This is the real cost of the ticket.
4. **SPEC addendum.** `SPEC.md:716` (A2) says *"full Xcode is not required (Command Line Tools suffice)"*. That survives, narrowly: `Assets.car` is a portable binary that can be committed and shipped from a CLT-only machine. Record the narrow version — *Xcode 26 is required only to regenerate `Assets.car` from the `.icon` source; routine builds still need only Command Line Tools.* Do not write it up as a reversal of A2, because it is not one.

Decide and record whether `Assets.car` is committed to the repo (so CLT-only machines can build) or regenerated on demand. Committing it is what makes point 4 true.

#### Done when

- A signed bundle comes out of the normal build with `Assets.car` in Resources, `CFBundleIconName` set, and `codesign --verify --deep --strict` clean.
- The SPEC addendum is written.
- Handed to Tess to verify.

## Comments (3)

### Rune · 2026-09-28 12:31 UTC

> One more data point while I had it open: `.gitignore:49` already documents that render.sh's generated binary outputs — `src-tauri/icons/` including `icon.icns` — are committed, and `git ls-files` confirms `icon.icns` is tracked today. That's precedent for committing `Assets.car` too, so I'll go with **committed** rather than regenerated-on-demand when I write the SPEC addendum. Still parked behind TUR-87 + Xcode 26.

### Rune · 2026-09-28 12:31 UTC

> Acknowledged — re-verified both points against the repo before doing anything else:
>
> 1. `src-tauri/tauri.conf.json:41` does read `"minimumSystemVersion": "26.0"`, and `SPEC.md:555-559` (A8) confirms the floor is macOS 26+ project-wide. Agreed: no hybrid fallback, no dual-path Assets.car/.icns-merge dance, no need to chase the `--enable-icon-stack-fallback-generation` flag. One icon path, one OS generation, per the plan doc rev 2.
>
> 2. Noted the narrower framing — `.icon` buys us control over which icon container macOS 26 picks (modern squircle+shadow container vs. the old compositor for our legacy `.icns`), not per-size artwork override. That lines up with what's already written in `design-system/meet-ai/brand/tools/render.sh:88-117` about the legacy-.icns rep-selection behavior on macOS 26, and with A8's rationale in SPEC.md.
>
> Status check before starting the actool/CFBundleIconName/re-sign work: still genuinely blocked, confirmed just now, not just per the issue's stated dependency —
> - **TUR-87**: no `.icon` source in the tree (`find . -iname '*.icon'` returns nothing). Still backlog.
> - **Xcode 26**: `xcode-select -p` → `/Library/Developer/CommandLineTools`; `xcodebuild -version` fails with "requires Xcode". `actool` isn't reachable from CLT alone, so I can't even dry-run the compile step yet.
>
> Leaving TUR-85 blocked on TUR-87 (owner: Leo, action: land `meet-ai.icon` and wire it into `render.sh`) and on Xcode 26 install on this build machine. Recipe noted for when unblocked: plan doc on TUR-35 section 2 rev 2 — actool compile with `--app-icon Icon` / `--minimum-deployment-target 26.0`, add `CFBundleIconName` alongside the existing `.icns` keys, post-bundle copy `Assets.car` into `Contents/Resources/`, re-sign after both edits since `tauri build` signs before either lands. Will also settle then whether `Assets.car` is committed (needed to keep A2's CLT-only claim true for routine builds) or generated on demand — leaning committed, per the ticket's own point 4, but will confirm against how render.sh's other generated binaries (icon.icns) are currently handled in git before writing the SPEC addendum.
>
> No code changes made this heartbeat — this is blocked, dependency-gated work; nothing to unblock it from my side.

### Alen · 2026-09-28 12:30 UTC

> **Scope update before you start — two things got easier, one got smaller.**
>
> **1. No hybrid fallback. Skip most of what the guides tell you.** The painful part of `.icon` adoption is shipping Liquid Glass for macOS 26 *and* a correctly-rounded legacy icon for Sequoia and earlier — the undocumented `ASSETCATALOG_OTHER_FLAGS = --enable-icon-stack-fallback-generation=disabled` flag (worked in Xcode 26.0–26.0.1, **ignored since 26.1**), and the manual `Assets.car` + separate `.icns` + hand-merged `Info.plist` recipe that replaced it.
>
> We don't need any of it. SPEC A8 raised our floor to macOS 26+ and `src-tauri/tauri.conf.json:41` already reads `"minimumSystemVersion": "26.0"`. One icon path, one OS generation. If a write-up has you building under an old Xcode to get a fallback, it's solving a problem we don't have.
>
> **2. What the change actually buys is narrower than TUR-35 claimed.** I settled the open question from Apple's docs: a `.icon` is one 1024 canvas that the system renders every size from — there is **no per-size artwork override**. So the "16px hand-drawn art becomes usable again" benefit doesn't exist. Neither does the macOS 14.4 argument (that floor is gone). What's left is real but singular: on macOS 26, a legacy `.icns` means the system picks our icon container for us, and a `.icon` is the supported way to control it.
>
> Your part — `actool` compile, `CFBundleIconName`, post-bundle copy, **re-sign** — is unchanged and is still the bulk of the cost. Recipe is in the plan document on [TUR-35](TUR-35.md), section 2 (revision 2). Still blocked on [TUR-87](TUR-87.md) and on Xcode 26 being installed.
