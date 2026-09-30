# TUR-86 — the Icon Composer `.icon` on a real signed bundle

Parent: TUR-35. Harness: `design-system/meet-ai/brand/tools/iconprobe/verify-icon.sh`
(see its README). Everything here was drawn through `NSWorkspace.icon(forFile:)`,
the path Finder and the Dock use, from re-signed lab copies with their own
bundle ids.

**Status: baseline captured 2026-09-30. The `.icon` column is pending** — it
needs the bundle from TUR-85 (the `actool`-compiled `Assets.car`, plus
`CFBundleIconName`, plus a re-sign).

Success is **parity**: every rung the same as today's `.icns` or better, no
rung worse, and the signature clean after the bundle is modified. Rungs that
come back indistinguishable are the expected result (TUR-35 plan, section 5),
not a failure.

## Conditions

| | Baseline | `.icon` |
|---|---|---|
| Bundle | `target/release/bundle/macos/meet-ai.app` in the main checkout, copied with `ditto` to `target/icon-lab/baseline/` | pending |
| Built / signed | Info.plist 30 Sep 2026 18:36:18, signed 18:36:19, `Authority=meet-ai Local Signing` | pending |
| `icon.icns` | `86a782d8…` — byte-identical to `src-tauri/icons/icon.icns` on this branch and on the main checkout; 6 reps, 128pt–512pt@2x (the TUR-22 ladder) | pending |
| `Info.plist` | `CFBundleIconFile = icon.icns`; no `CFBundleIconName`; no `Assets.car` | pending |
| macOS | 27.0 (26A428) | |
| System icon style | default (`AppleIconAppearanceTheme` unset) | |

One thing about the baseline bundle that is not about icons: its `Info.plist`
says `LSMinimumSystemVersion = 14.4`, not 26.0. `src-tauri/Info.plist` still
carried 14.4 and overrode `tauri.conf.json`'s `minimumSystemVersion: "26.0"`
(SPEC A8). This branch sets it to 26.0, so the `.icon` bundle should say 26.0.

## Hard checks

| Check | Baseline (`.icns` only) | `.icon` |
|---|---|---|
| `codesign --verify --deep --strict` | **PASS** | pending |
| `CFBundleIconFile` set and present | **PASS** | pending |
| `CFBundleIconName` set | FAIL — expected, not in this bundle | pending |
| `Assets.car` present | FAIL — expected | pending |
| `Assets.car` has the app icon, as `IconImageStack` | FAIL — expected | pending |
| Renders are the icon, not the generic fallback | **PASS**, all 13 rungs | pending |
| System draws `Assets.car`, not the `.icns` | FAIL — expected: the decoy `.icns` shows through at every rung (up to 75% of pixels) | pending |
| `.icns` fallback still works (Assets.car removed) | n/a — this *is* the fallback | pending |

How "draws `Assets.car`" is decided: `--reps` cannot answer it. On this OS it
lists the same synthesised ladder (16, 18, 24, 32, 128 … 1024, each at 1x and
2x) for our `.icns`-only bundle and for Terminal.app, which ships an
`Assets.car`. So the script swaps the `.icns` for a magenta decoy with the same
rep ladder. If the render does not move, the `.icns` is not being drawn. A
second copy with `Assets.car` removed as well must show the decoy, or the test
proves nothing.

**Calibration — Terminal.app** (ships both `Assets.car` and `Terminal.icns`,
`CFBundleIconName = Terminal`, and `IconImageStack` renditions in its catalog):
every check passes. The decoy `.icns` changes nothing (0.00%); with
`Assets.car` removed the decoy appears (75%). So the method can tell the two
paths apart. The `.icns`-only Terminal also renders visibly differently from
its catalog icon (up to 63% of pixels, mostly the glass rim), so expect the
`.icon` rungs to differ from the baseline, not just match it.
`tur86-G-calibration-terminal-trials-128.png`.

## Rungs

"Tile edge" is the first opaque pixel walking in from the left along the
middle row: our ink is about `rgb(35,38,45)`, and the system's light plate is
about `rgb(200,200,200)`. That plate is the TUR-22 defect: the tile shrunk and
set onto a light plate, a square inside a square.

| Rung | Where it shows up | Baseline | Tile edge | `.icon` | pixdiff vs baseline | Verdict |
|---|---|---|---|---|---|---|
| 16 | Finder list / column / sidebar, Open and Save panels (1x) | correct: dark tile to the container edge, brackets and dot resolve | `rgb(33,36,44)` | pending | pending | pending |
| 32 | menus, small Finder icons (1x) | correct | `rgb(34,34,44)` | pending | pending | pending |
| 16@2x | the 16pt places on Retina | correct | — | pending | pending | pending |
| 41 | the Dock at this user's tile size (1x) | correct | — | pending | pending | pending |
| 64 | Finder icon view (1x) | correct | — | pending | pending | pending |
| 32@2x | the 32pt places on Retina | correct | — | pending | pending | pending |
| 128 | Get Info, Quick Look (1x) | correct | `rgb(35,38,45)` | pending | pending | pending |
| 256 | large Finder icons | correct | — | pending | pending | pending |
| 128@2x | Get Info on Retina | correct | — | pending | pending | pending |
| 512 | largest Finder icon size (1x) | correct | `rgb(37,38,46)` | pending | pending | pending |
| 256@2x | large Finder icons on Retina | correct | `rgb(37,38,46)` | pending | pending | pending |
| 512@2x | largest Finder icon size on Retina, previews | **defect** — tile nested on a light system plate | `rgb(196,197,197)` | pending | pending | pending |
| 1024 | 1024 px at 1x (not a standard macOS size; kept as the top of the ladder) | **defect** — same plate | `rgb(196,197,197)` | pending | pending | pending |

Verdict key: **better** (visibly closer to the design or fixes a defect),
**same** (pixdiff `changed` at or under 0.5%, or a difference nobody can see
on the sheet), **worse** (any regression, and it blocks). A pixdiff
difference on its own does not count as a verdict. Look at the sheet.

**New finding: the baseline is not clean at 1024 px.** TUR-14 measured up to
256 and TUR-22 up to 128, and both were clean. At 512pt@2x and 1024pt@1x this
`.icns` still gets the old treatment: it is drawn inside a light system plate,
the same thing TUR-22 fixed at 16 and 32. The `.icns` rep itself is clean
(`icon_512x512@2x` extracted with `iconutil` is a clean dark tile with no plate). What changes
it is how macOS draws it. It shows up on Retina at the largest Finder icon
size and in large previews. It was not visible before because nobody measured
that rung. This is the one rung where the `.icon` could come back **better**
and not just the same. `tur86-E-baseline-1024-512x2-half.png`.

## Proofs

| File | What it shows |
|---|---|
| `tur86-A-baseline-16-32-16x2.png` | 16, 32, 16@2x, 8x nearest-neighbour, on dark and light |
| `tur86-B-baseline-41-64-32x2.png` | 41 (the Dock tile), 64, 32@2x, 4x |
| `tur86-C-baseline-128-256-128x2.png` | 128, 256, 128@2x, 1x |
| `tur86-D-baseline-512-256x2.png` | 512, 256@2x, 1x |
| `tur86-E-baseline-1024-512x2-half.png` | 1024, 512@2x: the plate. Downscaled to half for size, the only smoothed image here |
| `tur86-F-baseline-decoy-shows-through.png` | baseline vs. the same bundle with a decoy `.icns`: the decoy is drawn, so this bundle draws its `.icns` |
| `tur86-G-calibration-terminal-trials-128.png` | Terminal.app: shipped / decoy `.icns` / `Assets.car` removed / both. The decoy only appears once the catalog is gone |

## Not covered by this harness

- **Real Dock and Finder screen captures.** TUR-14 did those by hand. They need
  Screen Recording permission and a live Dock. On TUR-22, `NSWorkspace` renders
  matched Finder pixel for pixel, so the rungs stand in for them. A single
  Dock capture of the `.icon` build at the 41px tile is still worth taking
  once, to be sure.
- **Dark, tinted and clear icon styles.** These follow the user's system icon
  style setting (Appearance → Icon & widget style), not the drawing appearance.
  `sysicon` drawing under dark returns identical pixels. Measuring them means
  changing that setting, which this harness does not do. The `.icon`'s catalog
  listing (`assetutil`) at least shows which appearances were compiled in.
- **macOS below 26.** Not supported (SPEC A8).

## Filling in the `.icon` column

```sh
H=design-system/meet-ai/brand/tools/iconprobe
$H/verify-icon.sh /path/to/the/tur-85/meet-ai.app --label icon
```

It compares against `target/icon-lab/runs/baseline/`. If that folder is gone,
re-run the baseline first (`--label baseline` on
`target/icon-lab/baseline/meet-ai.app`, or on any bundle built from main).
Rendering is deterministic, so the baseline reproduces exactly. Paste
`runs/icon/summary.md` into the tables above, copy `runs/icon/sheets/*.png`
here as `tur86-H…`, and write a verdict for each rung by looking at the
sheets.
