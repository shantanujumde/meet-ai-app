# TUR-87 — Author the meet-ai.icon in Icon Composer and wire it into render.sh

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **backlog** |
| Priority | low |
| Owner | Leo |
| Created | 2026-09-28 06:11 UTC by Alen |
| Parent | [TUR-35](TUR-35.md) Decide whether to ship an Icon Composer .icon (needs Xcode 26, not installed) |

## Description

**Prerequisite: Xcode 26 must be installed.** The user is installing it; this ticket stays in backlog until `xcrun --find actool` resolves, then it moves to todo.

Full plan and the verified `actool` recipe: the **Icon Composer .icon — implementation plan** document on [TUR-35](TUR-35.md) (revision 2).

#### The open question is closed — answer is no, and it changes the goal

The previous version of this ticket asked you to spend ten minutes in Icon Composer answering whether a `.icon` supports per-pixel-size artwork. **Don't — I settled it from Apple's documentation on 2026-09-28.**

> "The system automatically renders your app icon for the different platforms, appearances, and sizes from your single Icon Composer file."
> — *Creating your app icon using Icon Composer*, Apple Developer

One 1024×1024 canvas, at most four layer groups, appearance variants for light / dark / tinted / clear. No per-size override exists in the format. `actool` emits 16/32/…/512 variants into `Assets.car`, but it **generates** them from the one composition.

So `design-system/meet-ai/brand/meet-ai-appicon-16-fullcolor.svg` **still never reaches the app icon.** That was TUR-35's headline benefit and it does not exist. Don't design around it. The file stays exactly as it is for the favicon and `.ico` outputs, which are correct and untouched.

**What this ticket is actually for, then:** on macOS 26 a legacy `.icns` means the system picks our icon container for us. A `.icon` is the supported way to control it. That is the whole remaining case — a platform-alignment change, not a fix.

#### Good news: skip the hybrid-fallback complexity

Most `.icon` migration guides spend their length on shipping a Liquid Glass icon for 26 *and* a correctly-rounded legacy icon for Sequoia and earlier — the `--enable-icon-stack-fallback-generation=disabled` flag, which broke in Xcode 26.1+, and the manual `Assets.car` + `.icns` merge that replaced it.

**None of it applies to us.** SPEC A8 (`SPEC.md:555`) raised the floor to macOS 26+ and `src-tauri/tauri.conf.json:41` already reads `"minimumSystemVersion": "26.0"`. One icon path, one OS generation, nothing to reconcile. If a guide has you doing the hybrid dance, it is solving a problem we do not have.

#### The work

1. Author `design-system/meet-ai/brand/meet-ai.icon` from the existing brand SVGs. Layered composition, light/dark/tinted/clear variants as the platform expects.
2. Wire it into `design-system/meet-ai/brand/tools/render.sh` **alongside** the `.icns`, not instead of it. Both ship — Tauri's bundler still wants the `.icns` and keeping it costs nothing.
3. **Keep the existing comment block** at `render.sh:82-127`. It explains the TUR-22 small-end decision and it is still accurate. Add a pointer to the `.icon` path; do not delete it.
4. Leave the `.ico` and favicon outputs untouched.

#### Done when

- `meet-ai.icon` exists in the brand directory and `render.sh` produces it.
- `render.sh` still produces an identical `.icns`, `.ico` and favicon set.
- Handed to Rune ([TUR-85](TUR-85.md)) for the bundle pipeline.

