# meet-ai — brand

The identity for a botless, local-first meeting recorder. Read this before you
put the mark anywhere.

**The symbol is "m." — a heavy white m with a full stop, on a Dusk gradient
tile, coral to violet. Adopted 2026-09-30**, replacing `[ · ]`, the dark tile
with two brackets and an ember dot that the board confirmed on 2026-09-27
(TUR-12). Why it changed, and what it was measured against, is in §10. The
wordmark is unchanged. This is the adopted identity; treat the rest of this
document as the rule, not a proposal.

Everything under this directory except `README.md` and the files listed below
is **generated**. The source of truth is `tools/geometry.mjs`;
`tools/build.mjs` writes the SVG masters and `tools/render.sh` writes the
rasters, the `.icns`, the `.ico`, the favicons and `meet-ai.icon`. Do not
hand-edit an SVG here — the next build overwrites it.

**Not generated, and not regenerable by these tools:**

- **Historical evidence of the `[ · ]` mark.** `proof-dock-signed-bundle.png`,
  `proof-dock-signed-bundle-zoom.png` and `proof-16px-qa-independent.png`
  (captures of a real signed `.app` in the real Dock and an independent 16px
  measurement, committed by QA); everything in `proofs/tur86/` (the TUR-86
  measurements of the real signed bundle, made with
  `tools/iconprobe/verify-icon.sh`); the `leo-tur22-*` and `leo-tur41-*`
  measurements; `proof-menubar-template-light-dark.png`; and
  `proof-directions.png`, the 2026-09-27 direction sheet. They all show the old
  mark, on purpose: they are the record of how the icon pipeline was proved,
  and the findings about the pipeline (which `.icns` reps to ship, how the
  `.icon` is compiled) still hold. A clean-and-rebuild will not recreate them —
  do not delete them on the assumption that the build will put them back.
- **The evidence for the 2026-09-30 decision.** `proof-competitors.jpg`,
  `proof-colour-concepts.jpg` and `proof-before-after.jpg` (§10), copied from
  the icon lab and compressed.

```
node tools/build.mjs        # SVG masters + brand-tokens.css + render manifest
./tools/render.sh           # PNGs, icon.icns, icon.ico -> src-tauri/icons, public/;
                            #   and meet-ai.icon (via tools/build-icon.mjs)
./tools/render-proof.sh     # the review proofs (after the two above)
just icon-car               # from the repo root: meet-ai.icon -> Assets.car (Xcode 26+)
```

`render-proof.sh` runs its own page generator, so it is safe to run on its own
once the rasters exist. It verifies what Chrome actually produced before
accepting it: a missing input page fails the script instead of being
screenshotted as an error page, and a render that comes out the right size but
empty is rejected rather than committed. See `tools/_shoot.zsh`.

---

## 1. The idea

**"m."** — the m of meet-ai, then a full stop.

The m is the wordmark's own first letter, built the same way (one monoline
stroke, round ends, circular arches) and set heavy. The dot is the record
light the identity has always had, now sitting on the baseline as a full stop:
the meeting, and the point at which it is written down and done. It is a
letter and a mark of punctuation, so it reads the same in one colour, in
greyscale and to colour-blind eyes.

It is also, deliberately, the kind of icon people tap: a bright tile with one
confident white glyph, the way social and messaging apps look in a Dock. The
old mark said "developer tool" and got lost among the dark squircles; this one
is meant to be found.

---

## 2. Files

| File | Use |
|---|---|
| `meet-ai-appicon-primary-fullcolor.svg` | **App icon master**, 1024 — Dusk tile, white "m." |
| `meet-ai-appicon-small-fullcolor.svg` | App icon, 20–64px (dot set a little further out) |
| `meet-ai-appicon-16-fullcolor.svg` | App icon, 16px, drawn pixel by pixel |
| `meet-ai-appicon-flat-fullcolor.svg` | App icon on one flat colour (print, no gradient) |
| `meet-ai.icon/` | Icon Composer document — the macOS 26 app-icon source that compiles to `Assets.car` (see §8) |
| `meet-ai-favicon.svg` | Web favicon — the small artwork on the Dusk tile |
| `meet-ai-logomark-primary-ink.svg` | Symbol alone on light backgrounds, 40px and up |
| `meet-ai-logomark-primary-chalk.svg` | Symbol alone on dark backgrounds, 40px and up |
| `meet-ai-logomark-small-chalk.svg` | Symbol alone at 20–32px |
| `meet-ai-logomark-mono-black.svg` / `-mono-white.svg` | One-colour symbol |
| `meet-ai-wordmark-primary-ink.svg` / `-chalk.svg` | Wordmark alone |
| `meet-ai-wordmark-mono-black.svg` / `-mono-white.svg` | One-colour wordmark |
| `meet-ai-logo-horizontal-ink.svg` / `-chalk.svg` | **Primary lockup** — "m." then the wordmark |
| `meet-ai-logo-horizontal-mono-black.svg` / `-mono-white.svg` | One-colour lockup |
| `meet-ai-menubar-template-black.svg` | macOS menu-bar template, 16×16 |
| `brand-tokens.css` | Colour tokens, with contrast computed from the palette |
| `proofs/` | Rendered review proofs (app icon and every `.icon` appearance, menu bar, logo system, in context, misuse), the §10 evidence, and the historical captures listed at the top |
| `concepts/` | The three directions explored on 2026-09-27, kept for the record. None is live |

The file names are the ones the `[ · ]` identity used, so nothing that points
at them had to change.

---

## 3. Colour

| Token | Value | Use |
|---|---|---|
| `--brand-tile-top` / `--brand-tile-bottom` | `#F46A3A` / `#9B3CF2` | **Dusk** — the app-icon tile, top to bottom |
| `--brand-tile-flat` | `#C85396` | Dusk on one colour: the gradient's midpoint, for the flat file and print |
| `--brand-glyph` | `#FFFFFF` | "m." on the tile |
| `--brand-ink` | `#16181D` | The tile in Dark appearance; one-colour dark |
| `--brand-ink-top` / `--brand-ink-bottom` | `#24262E` / `#121317` | Dark-appearance tile gradient |
| `--brand-chalk` | `#F4F5F7` | The symbol and wordmark on dark |
| `--brand-ember` | `#FF8A3C` | The accent off the tile: the free-standing symbol's dot, the wordmark's `i` tittle |
| `--brand-ember-core` / `--brand-ember-rim` | `#FFC152` / `#E85F21` | Kept for continuity; nothing generated uses them now |
| `--brand-ember-ink` | `#A8410D` | Ember as *text* on white |

On the tile the whole "m." is white. Off the tile, the m is ink or chalk and
the dot is ember — the same accent as the wordmark's tittle, so the symbol and
the word carry one colour idea between them.

Measured contrast (WCAG 2.1; `brand-tokens.css` computes these from the
palette on every build, so they cannot go stale):

| Pair | Ratio | Verdict |
|---|---|---|
| white on tile top | 3.0:1 | Just clears 3:1, the bar for graphics |
| white on tile bottom | 4.9:1 | AA large |
| white on tile flat | 4.1:1 | AA large |
| coral dot on ink (Dark appearance) | 5.9:1 | AA |
| chalk on ink | 16.3:1 | AAA |
| ink on white | 17.8:1 | AAA |
| ember on white | 2.3:1 | **Fails.** Never use ember as text on white |
| ember-ink on white | 6.1:1 | AA normal, AAA large |

### The top of the tile is the weak spot, and it was deepened

White on the approved coral, `#FF7A45`, is **2.6:1**. The top stop was
deepened to `#F46A3A` — the smallest step that puts the swatch at 3:1 — and it
still reads as coral, the same family as ember. What that buys, measured on
ictool's own render of the `.icon` rather than on the swatch, because the
system lightens the top of the tile with its own edge light:

| Behind the glyph, as macOS renders it | `#FF7A45` (approved) | `#F46A3A` (shipped) |
|---|---|---|
| above the m's shoulders, 1024px | 2.55:1 | 2.86:1 |
| mid-height, 1024px | 3.04:1 | 3.27:1 |
| above the shoulders, 32px | 2.55:1 | 2.87:1 |
| mid-height, 32px | 3.08:1 | 3.32:1 |

That is a real but modest gain. At 16px and 32px the glyph's legibility comes
mostly from its shape — three heavy legs, two open counters, a separate dot —
and the side-by-side renders look near-identical. **The trade-off, stated
plainly:** a white glyph on a light, warm colour cannot reach 4.5:1 without the
coral going brick-red and losing the look that was chosen. The shoulders of
the m sit at about 2.9:1. That is accepted, because the glyph is large and
heavy, and nothing depends on reading fine detail there.

Print references, for a press-ready file this environment cannot produce:
ink ≈ CMYK 78/70/60/70, Pantone Black 6 C. Ember ≈ CMYK 0/56/80/0,
Pantone 1575 C. Dusk has no press reference yet. All of these are
approximations from sRGB and must be proofed on press before anything is
printed; a gradient in particular needs a printer's proof.

### The brand colour does not enter the interface

`design-system/meet-ai/tokens.css` uses macOS system colours on purpose, and
`--accent` stays system blue. Dusk and ember live on the app icon, the symbol,
the README and marketing surfaces. Do not repoint `--accent` at a brand
colour.

---

## 4. Clear space and minimum size

**Clear space** on all four sides is the **diameter of the dot** at whatever
size the mark is being used. Nothing enters that margin — no text, no rule, no
other logo, no crop. The lockup puts two dot diameters between the symbol and
the word; at one they read as a single word, "m.meet-ai".

**Minimum sizes:**

| Asset | Minimum | Below that |
|---|---|---|
| Lockup | 120px wide | Use the symbol alone |
| Symbol alone, primary artwork | 40px | Switch to `-small-chalk` |
| Symbol alone, small artwork | 20px | Use the app icon instead |
| App icon | 16px | Fixed; the 16px file is drawn for exactly this size |
| Menu-bar template | 16px | Fixed; drawn for exactly this size |

The app icon exists in **three drawings**, not one scaled:

- **The master** (128px and up, and the `.icon`): the approved geometry.
- **The small artwork** (20–64px, favicon): the same m with the dot 16 units
  further out, so a whole pixel of tile stays between the dot and the last
  leg at 20px.
- **The 16px icon**, drawn pixel by pixel: legs 2px wide on whole pixels,
  counters 1px, the shoulders two rows deep with their outer corners at half
  strength, and a solid 2×2 dot one clear pixel from the last leg. Drawn with
  the round construction, the shoulders and dot came out as half-strength
  pixels and the dot all but vanished.

The menu-bar template is a fourth, separate drawing (§8). Use the right one;
do not scale the wrong one and hope.

---

## 5. Backgrounds

**The app icon carries its own background** — the Dusk tile — and goes
anywhere an app icon goes.

**The symbol alone** goes on **ink**, on **paper (white)**, or on a **flat
neutral**: `-chalk` on dark, `-ink` on light, the `-mono-` files when only one
colour is available. Never on a gradient, a photograph or a busy screenshot. If
you want "m." on colour, use the app icon; do not put the bare symbol on
something colourful and call it the icon.

---

## 6. Do not

The rendered failure cases are in `proofs/proof-misuse.png`. In words:

- **Do not stretch it.** The stroke stops being uniform and the dot becomes an
  ellipse. Scale proportionally only.
- **Do not recolour the tile.** Dusk is the identity. Blue and purple on their
  own are exactly the meeting-notes field's colours (§10).
- **Do not recolour the dot on its own.** On the tile it is white with the m.
  Off the tile it is ember. In Dark appearance it is coral. Nothing else.
- **Do not add effects.** No drop shadow, bevel, outer glow or glass on the
  glyph. The `.icon` has Liquid Glass off for this reason (§8).
- **Do not move the dot.** It is a full stop on the baseline. Lifted to where
  an `i`'s tittle sits, "m." becomes a white "mi" — Xiaomi's old mark (§9).
- **Do not rotate it.** It is a letter; it reads upright.
- **Do not rebuild the lockup.** Use the supplied file. Do not set "meet-ai" in
  a system font beside the mark — the wordmark is drawn, and a system font
  next to it is immediately visible as wrong.
- **Do not crowd it.** Honour the clear space.
- **Do not put the mark on glass.** It is a solid object. `glass.css` surfaces
  carry it on an opaque plate or not at all.

---

## 7. Typography

**The wordmark is drawn, not set.** One monoline construction: one stroke
weight, round terminals, circular bowls, and the `i` tittle is the record dot.
There is no typeface behind it. The symbol's m is this wordmark's m at a
heavier weight — the same function in `geometry.mjs` draws both.

That is deliberate. Apple's SF Pro would have matched the product's UI, but the
SF licence covers interface mock-ups for Apple platforms and does **not** permit
use in a logo or wordmark. Drawing the wordmark removes the question entirely.

**For supporting text** in any brand context, use the system stack already in
`design-system/meet-ai/tokens.css`:

```
--font-ui: -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif;
```

**No font file ships with this identity and no font licence is required
anywhere.** If that ever changes, the new face's commercial-use and embedding
terms must be stated here before it is used.

---

## 8. macOS specifics

The legacy app-icon SVGs follow Apple's macOS icon grid: a 1024 canvas with an
824 body centred in it. The 100px margin is the grid's shadow room, not
padding to fill. The corner is a **superellipse** (n = 4.6), not a rounded
rectangle — a plain `rx` corner reads subtly wrong beside native icons because
the arc starts too late and ends too abruptly. The "m." is drawn once, on the
1024 Icon Composer canvas where the canvas is the tile, and the legacy files map
that whole canvas onto the 824 body, so the glyph takes the same share of the
tile in both formats by construction.

Depth comes from the gradient and one specular rim clipped to the body (light
at the top, dark at the bottom — the same rim language as `glass.css`). There
is deliberately no drop shadow under the glyph.

The menu-bar asset is a **template image**: pure black with alpha. macOS tints
it, so it carries no brand colour. It is drawn on the 16px pixel grid — legs
2px on whole pixels, counters 2px, a 3px dot with a clear pixel before it —
and every coordinate doubles onto whole pixels, so the 2x raster is the same
drawing at twice the grid. The 2x is the one `src-tauri/src/tray.rs` embeds.
The template has no recording state; the menu and the popover say what is
happening.

`meet-ai-menubar-template-black.svg` here is the **master**, not something the
app can load. Whatever rasterises it must name the output with AppKit's
`…Template.png` / `…Template@2x.png` suffix, or set the template flag
explicitly. An unflagged PNG renders solid black on a dark menu bar, which
reads as a bug rather than a choice. `render.sh` does the former and `tray.rs`
the latter.

### The Icon Composer `.icon` (TUR-87)

`meet-ai.icon/` is the app icon in Apple's current format: a folder holding
`icon.json` and the layer art under `Assets/`. On macOS 26 it is how an app
chooses its own icon container instead of having one applied to a legacy
`.icns`. It ships **alongside** `icon.icns`, not instead of it.

It is generated like everything else here. `tools/build-icon.mjs` writes it
from `geometry.mjs`, and `render.sh` runs that step, so do not edit it in Icon
Composer and commit the result — the next render overwrites it. Writing it needs
only node.

What is in it, and why:

- **One 1024 composition, no per-size art.** The format has no way to supply a
  drawing for a particular size; the system renders 16px through 1024px from
  this one file. The 16px drawing reaches only the favicon and `.ico`. That is
  why the master's dot is 60 units from the last leg, not the prototype's 44:
  at 44, ictool's 16px render fused the dot onto the leg in the Default,
  Tinted and Clear appearances, and "m." read as an m with a foot.
- **The tile is the document fill, the glyph is two layers** (dot, m). The
  squircle, the grid margin, the edge highlight and the shadow are the
  system's, so none of them are drawn.
- **Dusk runs straight down.** The prototype asked for a slight diagonal, but
  ictool renders a document fill vertically whatever the x offsets say
  (measured: the left and right edges match at every height). The approved
  look is therefore vertical, and the legacy SVGs, whose renderer does honour
  the diagonal, are written vertical to match it.
- **Liquid Glass is off**, as in the approved prototype. On the previous mark,
  glass made the system outline the glyph in a dark keyline and dim it
  (TUR-87) — a drop shadow and an effect on a solid object, §6, applied by the
  system. Off, the glyph is flat white.
- **Dark: ink tile, the colour moves into the glyph.** The m wears Dusk on a
  diagonal (a layer fill does honour x): coral at the left shoulder, violet at
  the right foot. The dot is solid coral, the record light — the prototype
  gave the dot its own copy of the whole gradient, which read as a second,
  smaller icon, and a coral dot also stays distinct from the violet foot
  beside it at 16px.
- **Tinted and clear are plain white**, pinned explicitly on both layers, so
  the system's one-colour rendering gives the glyph the full tint.

To check the document, `render.sh` uses Icon Composer's own renderer,
`ictool`, which ships inside Xcode. When it is present it confirms the document
parses and leaves a 1024 preview at `tools/.render/meet-ai-icon-preview.png`;
`render-proof.sh` renders every appearance into `proofs/proof-app-icon.png`.
Without Xcode both steps are skipped and the document is still written. By
hand:

```
ICTOOL="/Applications/Xcode.app/Contents/Applications/Icon Composer.app/Contents/Executables/ictool"
"$ICTOOL" meet-ai.icon --export-image --output-file out.png --platform macOS \
  --rendition Default --width 128 --height 128 --scale 2
# --rendition: Default Dark TintedLight TintedDark ClearLight ClearDark
```

**Compiling it into `src-tauri/icons/Assets.car` needs Xcode 26 or later**
(`actool`), which is why the compiled catalog is committed: routine builds stay
on Command Line Tools. Re-run `just icon-car` only when this document changes.
`actool` names the icon after the folder, so its `--app-icon` value, and
`CFBundleIconName` in the app's `Info.plist`, are both **`meet-ai`** — keep the
folder name.

### What `tauri.conf.json` actually reads

It lists **five** of the eight files in `src-tauri/icons/`:

| Referenced by `tauri.conf.json` | Present but unreferenced |
|---|---|
| `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `Assets.car` | `64x64.png`, `icon.png`, `icon.ico` |

The unreferenced three are stock Tauri defaults and harmless. For the Dock and
Finder, macOS reads `Assets.car` (named by `CFBundleIconName`) and falls back
to the `.icns` (named by `CFBundleIconFile`); Tauri copies both into the bundle.
`.ico` is Windows-only on a macOS-26-and-up product. Recorded so nobody later
reads "the config already points at these paths" as "all eight are wired". If
you regenerate the set, regenerate all seven `render.sh` outputs anyway: the
build pipeline and the `.ico` depend on them. `Assets.car` is the eighth, and
comes from `just icon-car`.

The `.icns` still leaves out its 16pt and 32pt reps (TUR-22; see `render.sh`):
that finding was about how macOS 26 treats a legacy `.icns`, not about the
artwork, so it carried over to the new mark unchanged.

### Reduced transparency and reduced motion

The identity has no motion and no translucency, so both settings are no-ops for
it. That is the point: when `glass.css` falls back to opaque surfaces, the mark
is unchanged.

---

## 9. Known risks

- **Trademark is not cleared.** No search has been run, for the name or for
  this mark. A proper clearance search is required before any public use.
  Until then "m." is a working identity, not a registered one.
- **Closest resemblance: Xiaomi's old "mi".** A white lower-case m on an
  orange tile, with a dot, sits near Xiaomi's pre-2021 logo — a white "mi" on
  orange, the dot being the i's tittle. The approved concept already moved
  the dot off the tittle position to a full stop on the baseline, and the
  tile runs to violet rather than staying flat orange; §6 forbids moving it
  back. Worth checking properly in the trademark search.
- **Letter-on-a-tile is a common genre.** Jamie ("J" on purple), Bluedot ("b"
  on blue), Krisp ("K") and Notion ("N") all use a single letter; "m." stands
  apart on colour and on the full stop, not on the idea of a letter.
- **The gradient has neighbours outside the field.** Instagram's sunset
  gradient and Claude's flat coral are both warm and bright. Dusk is a
  two-stop coral-to-violet with one white glyph, which is visibly different
  from either at Dock size (`proof-competitors.jpg`), but it is in the same
  family of colour as Instagram's, and that is the social-app look that was
  asked for.
- **Formats not produced here.** No `.ai`, `.eps`, or Pantone-accurate proof.
  The SVG masters are the source; a print vendor can open them directly, and
  the Pantone references above are approximations that need a press proof.

---

## 10. Decision record: why "m." (2026-09-30)

**What was asked.** The user wanted to move away from the dark tile with
brackets to something "more colourful and attractive, like a social-media
logo" — a bright, friendly icon rather than a developer-tool one.

**What the field looks like** (`proofs/proof-competitors.jpg`, the real icons
at 128 / 32@3x / 32). The AI meeting-notes apps are **blue, purple, black,
lime or pink**, with **letter or waveform glyphs**: Otter's blue waveform
wordmark, Fireflies' pink F, Fathom black, tl;dv and Zoom blue, Krisp black,
Jamie purple, Bluedot blue, Granola lime. Voice apps are black with
waveforms. The old `[ · ]` tile sat with the black ones.

**The concepts** (`proofs/proof-colour-concepts.jpg`): A, a speech bubble with
a record light; B, "m."; C, a glass record light; D, two overlapping bubbles —
each as a real `.icon`, rendered by ictool at every size and appearance, and in
a mock Dock between real app icons. Two gradients were tried across them,
Sunrise (amber to raspberry, on A and C) and Dusk (coral to violet, on B and
D). **B on Dusk was chosen.**

**Why Dusk.** It starts from the brand's own orange — the coral top is the
ember family, so the identity keeps its one warm colour — and runs to violet,
a combination nobody in the meeting-notes field uses. It reads as colourful and
social without borrowing a competitor's blue or purple outright.

**Why "m.".** It survives the small sizes best. At 16px the bubbles lose their
tails and read as blobs, and the glass light shrinks to a ring; the heavy m
keeps three legs and two open counters, and the dot stays a dot. It is also the only
concept that is a *letter*, so it ties straight to the wordmark.

**What changed from the prototype when it went into the pipeline**, each for a
measured reason: the dot is 60 units from the m instead of 44 (§8, it fused at
16px); the top stop is `#F46A3A` instead of `#FF7A45` (§3, contrast); the tile
gradient is written vertical, which is how ictool had already rendered the
prototype (§8); the Dark-appearance dot is solid coral instead of a miniature
gradient (§8). `proofs/proof-before-after.jpg` shows the old icon, the
approved prototype and the shipped result side by side.

**What was retired with the brackets.** The viewfinder proximity probe
(`tools/proximity.mjs`, `render-proximity.sh`, `proof-proximity.png`) existed
to keep `[ · ]` from reading as a camera focus target; it has nothing to
measure now and was deleted (it is in git history). The bracket misuse proof
was replaced by one for "m.". The bracket geometry itself is kept at the
bottom of `geometry.mjs`, clearly marked retired, only so `concepts.mjs` and
`tune.mjs` can still redraw the historical record.

**Still open.** The trademark search (§9).
