# TUR-86 — the Icon Composer `.icon` on a real signed bundle

Parent: TUR-35. Harness: `design-system/meet-ai/brand/tools/iconprobe/verify-icon.sh`
(see its README). Everything here was drawn through `NSWorkspace.icon(forFile:)`,
the path Finder and the Dock use, from re-signed lab copies with their own
bundle ids.

## Verdict: pass

**Better at 1 rung, the same at 12, worse at none.** The signature is clean
after the bundle is modified. The system draws the `.icon` and not the
`.icns`, and the `.icns` fallback is unchanged.

- **Better at 512pt@2x (and 1024@1x):** the `.icon` fixes a defect in today's
  `.icns` that nobody had measured before. At that size the baseline tile is
  drawn nested inside a light system plate, a square inside a square. The
  `.icon` draws a full dark tile.
- **The same everywhere else, 16px and 32px included.** The tile size and edge
  colour are identical. The differences pixdiff finds (3–21% of pixels) come
  from the system's container treatment: a fainter glass rim on dark
  backgrounds, and a slightly weaker ember halo at 512. The mark does not change.
- **32px at 1x is slightly softer:** about 20% fewer bright bracket pixels.
  That was expected, because a `.icon` has no per-size artwork. I do not count
  it as worse. The brackets and dot resolve as cleanly as before and the shape
  is unchanged. It shows side by side at 8x, and not at 1x.

Success was defined as parity (TUR-35 plan, section 5). This result is parity
plus one fix at the large end.

## Conditions

| | Baseline | `.icon` |
|---|---|---|
| Bundle | `target/release/bundle/macos/meet-ai.app` in the main checkout, copied with `ditto` to `target/icon-lab/baseline/` | `target/release/bundle/macos/meet-ai.app` in the TUR-35 worktree (TUR-85 build) |
| Built / signed | Info.plist 30 Sep 2026 18:36:18, signed 18:36:19, `Authority=meet-ai Local Signing` | Info.plist 30 Sep 2026 18:59:38, signed 18:59:39, `Authority=meet-ai Local Signing` |
| `icon.icns` | `86a782d8…`, 6 reps, 128pt–512pt@2x (the TUR-22 ladder) | `86a782d8…`, the same file |
| `Info.plist` | `CFBundleIconFile = icon.icns`; no `CFBundleIconName` | `CFBundleIconFile = icon.icns`, `CFBundleIconName = meet-ai` |
| `Assets.car` | none | 1,737,816 bytes; `meet-ai` as `IconImageStack` (Aqua, DarkAqua, Tintable) + `Icon Image` + `MultiSized Image` |
| `LSMinimumSystemVersion` | 14.4 (a stale `src-tauri/Info.plist` override) | 26.0 (SPEC A8) |
| macOS | 27.0 (26A428) | same machine |
| System icon style | default (`AppleIconAppearanceTheme` unset) | same |

## Hard checks

| Check | Baseline (`.icns` only) | `.icon` |
|---|---|---|
| `codesign --verify --deep --strict` | **PASS** | **PASS**, after the bundle was modified and re-signed |
| `CFBundleIconFile` set and present | **PASS** | **PASS** |
| `CFBundleIconName` set | FAIL — expected, not in this bundle | **PASS** — `meet-ai` |
| `Assets.car` present | FAIL — expected | **PASS** |
| `Assets.car` has the app icon, as `IconImageStack` | FAIL — expected | **PASS** |
| Renders are the icon, not the generic fallback | **PASS**, all 13 rungs | **PASS**, all 13 rungs |
| System draws `Assets.car`, not the `.icns` | FAIL — expected: the decoy `.icns` shows through (75% of pixels) | **PASS** — the decoy `.icns` changes 0.00%; the control with `Assets.car` removed shows it (75%) |
| `.icns` fallback still works (Assets.car removed) | n/a — this *is* the fallback | **PASS** — identical to the baseline at every rung (0.00%) |

How "draws `Assets.car`" is decided: `--reps` cannot answer it. It returns
the same synthesised ladder for both bundles, before and after (16, 18, 24,
32, 128 … 1024, each at 1x and 2x). So the script swaps the `.icns` for a
magenta decoy with the same rep ladder. If the render does not move, the
`.icns` is not being drawn. A second copy with `Assets.car` removed as well
must show the decoy, or the test proves nothing. `tur86-M`, `tur86-N`.

**Calibration — Terminal.app** (ships both `Assets.car` and `Terminal.icns`,
`CFBundleIconName = Terminal`, with `IconImageStack` renditions in its
catalog). Every check passes the same way: the decoy makes 0.00% difference,
and the control without the catalog shows it (75%). Its two paths differ by up
to 63% of pixels, mostly the glass rim. Ours differ by up to 45%, and outside
the 1024 plate by 3–21%. `tur86-G`.

## Rungs

"Tile edge" is the first opaque pixel walking in from the left along the
middle row: our ink is about `rgb(35,38,45)`, and the system's light plate is
about `rgb(200,200,200)`. "Chalk" counts pixels brighter than luminance 180,
which roughly measures how solid the brackets read. The tile span (the opaque
width of the middle row) is identical for both bundles at every rung, so the
`.icon` changes the container treatment, not the geometry.

| Rung | Where it shows up | Baseline | `.icon` | Tile edge (base → `.icon`) | Chalk (base → `.icon`) | pixdiff changed | Verdict |
|---|---|---|---|---|---|---|---|
| 16 | Finder list / column / sidebar, Open and Save panels (1x) | correct: dark tile to the container edge, brackets and dot resolve | the same mark; no faint light rim on dark | `33,36,44` → `33,34,42` | 16 → 16 | 21.09% | **same** |
| 32 | menus, small Finder icons (1x) | correct | brackets a touch thinner and greyer; still clean, dot identical | `34,34,44` → `33,34,42` | 60 → 48 | 11.52% | **same** — slightly softer, not worse (see verdict) |
| 16@2x | the 16pt places on Retina | correct | the same | `34,37,45` → `33,34,42` | 78 → 80 | 13.77% | **same** |
| 41 | the Dock at this user's tile size (1x) | correct, bright hairline rim on dark | the same mark; the rim is fainter | `35,38,47` → `35,38,46` | 104 → 106 | 16.00% | **same** |
| 64 | Finder icon view (1x) | correct | the same; fainter rim | `34,36,44` → `31,33,41` | 296 → 294 | 7.50% | **same** |
| 32@2x | the 32pt places on Retina | correct | the same | `34,36,44` → `31,33,41` | 296 → 294 | 7.50% | **same** |
| 128 | Get Info, Quick Look (1x) | correct | the same | `35,38,45` → `35,38,46` | 1204 → 1218 | 6.34% | **same** |
| 256 | large Finder icons | correct | the same | `35,38,45` → `38,41,49` | 5047 → 4995 | 3.23% | **same** |
| 128@2x | Get Info on Retina | correct | the same | — | — | 4.93% | **same** |
| 512 | largest Finder icon size (1x) | correct | the same; ember halo slightly weaker, rim thinner | `37,38,46` → `41,42,49` | 19852 → 19758 | 2.72% | **same** |
| 256@2x | large Finder icons on Retina | correct | the same | — | — | 2.71% | **same** |
| 512@2x | largest Finder icon size on Retina, previews | **defect** — tile nested on a light system plate | **correct** — full dark tile, no plate | `196,197,197` → `39,42,50` | (plate) → 79411 | 44.97% | **better** |
| 1024 | 1024 px at 1x (not a standard macOS size; the top of the ladder) | **defect** — the same plate | **correct** | `196,197,197` → `39,42,50` | (plate) → 79411 | 44.97% | **better** |

Verdict key: **better** (visibly closer to the design, or fixes a defect),
**same** (the mark and the container read the same on the sheet, whatever
pixdiff says), **worse** (any regression, and it blocks). A pixdiff number on
its own is never a verdict.

**About the 1024 plate.** TUR-14 measured up to 256 and TUR-22 up to 128, and
both were clean. At 512pt@2x and 1024@1x today's `.icns` still gets the old
treatment and is drawn inside a light plate, the same thing TUR-22 fixed at 16
and 32. The `.icns` rep itself is clean (`icon_512x512@2x` extracted with
`iconutil` is a clean dark tile with no plate). What changes it is how macOS
draws it. The `.icon` does not have the problem. The `.icns` fallback still
does, as expected, since that file did not change. `tur86-L`.

**About the fainter rim.** On dark backgrounds the baseline tile carries a
bright hairline rim, and the `.icon`'s is subtler. Terminal.app behaves the
same way: its `Assets.car` icon has a quieter rim than its own `.icns`. So this
is the system's native container for a `.icon`, not a defect in ours, and it is
neutral in every verdict above.

## Proofs

| File | What it shows |
|---|---|
| `tur86-H-icon-vs-base-16-32-16x2.png` | base / `.icon` pairs at 16, 32, 16@2x, 8x nearest-neighbour, on dark and light |
| `tur86-I-icon-vs-base-41-64-32x2.png` | 41 (the Dock tile), 64, 32@2x, 4x |
| `tur86-J-icon-vs-base-128-256-128x2.png` | 128, 256, 128@2x, 1x |
| `tur86-K-icon-vs-base-512-256x2.png` | 512, 256@2x, 1x |
| `tur86-L-icon-vs-base-1024-512x2-half.png` | 1024, 512@2x: the plate in the baseline and gone in the `.icon`. Downscaled to half for size, the only smoothed image here |
| `tur86-M-icon-trials-32.png` | `.icon` bundle at 32: shipped / decoy `.icns` / `Assets.car` removed / both. The decoy only appears once the catalog is gone |
| `tur86-N-icon-trials-128.png` | the same trials at 128 |
| `tur86-F-baseline-decoy-shows-through.png` | the baseline with a decoy `.icns`: the decoy is drawn, so that bundle draws its `.icns` |
| `tur86-G-calibration-terminal-trials-128.png` | Terminal.app, the same four trials: the method tells the two paths apart |

## Not covered by this harness

- **Real Dock and Finder screen captures.** TUR-14 did those by hand. They need
  Screen Recording permission and a live Dock. On TUR-22, `NSWorkspace` renders
  matched Finder pixel for pixel, so the rungs stand in for them. A single
  Dock capture of the `.icon` build at the 41px tile is still worth taking
  once, to be sure.
- **Dark, tinted and clear icon styles.** These follow the user's system icon
  style setting (Appearance → Icon & widget style), not the drawing appearance.
  `sysicon` drawing under dark returns identical pixels, and this harness does
  not change that setting. The catalog does contain `DarkAqua` and `Tintable`
  renditions of `meet-ai`, so the variants were compiled in. They have not
  been looked at.
- **macOS below 26.** Not supported (SPEC A8).

## Re-running

```sh
H=design-system/meet-ai/brand/tools/iconprobe
$H/verify-icon.sh target/icon-lab/baseline/meet-ai.app --label baseline   # if runs/baseline is gone
$H/verify-icon.sh target/release/bundle/macos/meet-ai.app --label icon
```

Rendering is deterministic on one machine, so both runs reproduce exactly.
