# meet-ai — Design System

**Platform:** macOS 26+ (Tahoe). **Stack:** Tauri 2 + React 19 + shadcn/ui, per SPEC.md.
**Language:** Apple Liquid Glass.
**Status:** v1. One user. Windows port is a seam, not a target (SPEC L1).

Read this file before building any surface. Page-level deviations live in
`pages/<page-name>.md` and override this file where they conflict.

Files:

| File | Holds |
|---|---|
| `tokens.css` | Three token layers: primitive, semantic, component |
| `glass.css` | Glass recipes, scroll edges, accessibility fallbacks |
| `contrast.mjs` | WCAG check of every text colour on every surface, all modes |
| `specimen.html` | The tokens and components, live, with theme and glass toggles |
| `MASTER.md` | Rules, component specs, and what not to do |

**Look (TUR-102).** Calm and roomy, after a Mac settings window: a glass
sidebar, a big bold page title with lots of space, rounded cards a step off
a brand-tinted canvas, one row pattern, one accent, rounded corners
everywhere. Light or dark is `<html data-theme>`, always set at runtime by
`src/lib/appearance.ts` from the saved choice (System follows the OS);
`data-glass="off"` makes every see-through surface solid.

---

## 1. What Liquid Glass actually is

Liquid Glass is a **layer**, not a fill. Apple splits the interface in two.
The content layer holds your work: notes, transcript, meeting list. The glass
layer floats above it and holds controls: toolbars, popovers, sheets. Glass
never becomes the content, and content never becomes glass.

Six properties define the look:

1. **Translucency with saturation lift.** Glass samples the backdrop and pushes
   its saturation up. A blur without the saturation boost reads as gray sludge.
2. **Lensing.** Real glass refracts hardest at its edges, so the backdrop bends
   near the rim rather than blurring evenly.
3. **Specular rim.** A bright highlight along the top edge, a dark one along the
   bottom. This single detail separates glass from a blurred rectangle.
4. **Adaptive tint.** The material shifts light or dark based on what is behind
   it, without the app choosing.
5. **Concentricity.** Nested corners share a center. An inner radius equals the
   outer radius minus the padding between them.
6. **Reactivity.** Glass responds to the pointer by brightening and compressing.
   It does not animate its blur.

### The honest limitation

Apple ships Liquid Glass as a native API. A web view cannot call it. This system
reproduces the observable properties in CSS and takes the genuine material where
the platform offers it:

| Property | How meet-ai gets it | Fidelity |
|---|---|---|
| Window material | Tauri `windowEffects`, a real `NSVisualEffectView` behind a transparent web view | Native |
| Blur and saturation | `backdrop-filter` | Very close |
| Specular rim | Masked gradient ring | Close |
| Lensing | Second masked backdrop pass at double blur | Approximate, and costs a GPU pass |
| Adaptive tint | `prefers-color-scheme` plus alpha fills | Coarse, switches per theme rather than per backdrop |
| Morphing between controls | View Transition API | Close where supported |

Do not claim the app uses Liquid Glass in a way that implies the native API.
It follows the design language over the platform material.

---

## 2. Non-negotiable rules

**Never stack glass on glass.** Two blurs compound into an unreadable smear and
double the compositing cost. Sibling controls share one `.glass-group` container
and become plain fills inside it.

**One tinted control per window.** The tint marks the single primary action. In
meet-ai that is Record while idle, and Stop while recording. Everything else is
neutral glass.

**Clear glass always needs a scrim.** `.glass--clear` guarantees no contrast on
its own. The dimming layer is built into the recipe, so do not strip it when the
backdrop happens to look dark during development.

**Text never lives on glass.** Body copy sits on the content layer. Glass carries
labels, icons, and short controls only. The one exception is the live transcript,
which is short-lined, high-contrast, and dismissible.

**Concentric radii are computed.** Write `calc(var(--radius-panel) - var(--pad))`.
A hand-typed inner radius will drift the moment padding changes.

**System font only.** `-apple-system` resolves to SF Pro and inherits optical
sizing. A Mac app shipping Plus Jakarta Sans or Inter reads as a website in a
window frame.

**Base size is 13px, not 16px.** macOS is a pointer platform with a shorter
viewing distance than the web assumes. Web habits will make the app look inflated.

**Brand tint on backgrounds only (TUR-102).** The canvas, sidebar and card
surfaces are neutrals nudged a few percent toward the brand: near-white with
a faint coral-violet in light, Ink nudged toward Dusk violet in dark
(`--tint-*` in tokens.css, each with its mix written beside it). The accent
stays macOS system blue. Never tint text, and never repoint `--accent`.

**Every text colour holds AA on every surface.** `node
design-system/meet-ai/contrast.mjs` composes each surface over its backdrop
and checks every text token on it in light, dark, Increase Contrast, Reduce
Transparency and glass off; `src/test/contrast.test.ts` fails CI if one pair
drops under 4.5:1 (3:1 for icons and the switch). White text never sits on the
raw system blue (4.0:1): filled controls with words use `--accent-fill`, words
in the accent use `--accent-text`, and the status words use the
`--status-*-text` shades.

---

## 3. Window setup

The web view must be transparent for the native material to show through.

```jsonc
// tauri.conf.json
{
  "app": {
    "windows": [{
      "transparent": true,
      "titleBarStyle": "Overlay",
      "hiddenTitle": true,
      "windowEffects": {
        "effects": ["underWindowBackground"],
        "state": "followsWindowActiveState",
        "radius": 12
      },
      "minWidth": 640,
      "minHeight": 420
    }]
  }
}
```

Notes that will cost time if missed:

- `transparent: true` on macOS requires `macOSPrivateApi: true` in the Tauri config.
- Use `titleBarStyle: "Overlay"`, not `"Transparent"`. Tauri's `"Transparent"` only
  makes the native title bar's own background see-through — it does not extend the
  web view under it, so the traffic lights end up sitting in a bare strip above your
  content instead of inset into it. `"Overlay"` is the one that merges the two.
- The traffic lights sit at the top left. Reserve `--titlebar-traffic-inset`
  (78px) or your own controls will collide with them.
- Any element in the titlebar needs `-webkit-app-region: drag` to move the
  window, and every button inside it needs `no-drag` back, or it stops clicking.
- `followsWindowActiveState` dims the material when the window loses focus,
  which is the macOS convention. Keep it.

---

## 4. Type scale

| Token | Size | Weight | Used for |
|---|---|---|---|
| `--text-title1` | 28px | 700 | Meeting title on the detail view |
| `--text-title2` | 22px | 600 | Section headings in notes |
| `--text-title3` | 17px | 600 | Panel headers |
| `--text-headline` | 15px | 600 | Emphasized rows, active meeting |
| `--text-callout` | 14px | 400 | Notes body |
| `--text-body` | 13px | 400 | Default UI text |
| `--text-footnote` | 12px | 400 | Transcript lines, metadata |
| `--text-caption1` | 11px | 400 | Timestamps, speaker labels |
| `--text-caption2` | 10px | 500 | Badges, counts |

Line length in the notes pane follows the page column, like the cards above it
(TUR-103: a 68ch cap left its right edge out of line with them); `--notes-measure`
(68ch) still caps notices and empty-state text. Transcript
lines are short by nature and need no cap.

Use tabular figures for timestamps so the transcript does not shift as the clock
ticks: `font-variant-numeric: tabular-nums`.

---

## 5. Component specs

### 5.1 Sidebar

See-through glass on macOS: `--surface-sidebar` (the sidebar tint at 66%, 60%
in dark) over the native window material, a clear tone off the content. It gets **no**
`backdrop-filter` of its own, because the window material is already the blur.
Solid (`--surface-sidebar-solid`) with glass off, Reduce Transparency, Increase
Contrast, and on Windows and Linux, where no material sits behind it.

- Width `--sidebar-w` (240px), padding `--sidebar-pad`
- A rounded find box at the top (capsule, `--surface-control`, search icon)
- Pages grouped under small grey headings (12px `--text-footnote`, semibold,
  secondary) that fold shut: a button with a chevron and `aria-expanded`
- Every row is a 16px accent line icon (`--accent-text`) and a name. Height
  `--sidebar-row-h` (30px), radius `--sidebar-row-radius`, hover
  `--sidebar-row-hover`
- The current page is a solid `--accent-fill` pill with white words and icon,
  and `aria-current`, never colour alone
- Meeting rows: title (13px medium, truncates), date and state on one line
  (12px, truncates). A recording with no transcript is dimmed: title
  secondary, icon and meta tertiary
- Recording meetings show the dot and the word, not a red title

### 5.1a Title bar

Slim, opaque (`--titlebar-fill` is the canvas), `--titlebar-h`: traffic-light
inset, back and forward arrows (icon buttons named "Back" and "Forward",
disabled when there is nowhere to go), the window title, the record control.

### 5.1b Pages, cards and the row pattern

- Page: big bold title (`--text-title1`, 700), `--page-pad-top` /
  `--page-pad-x` around it, `--page-gap` between sections, max `--page-max`
- Section: a 15px semibold heading over one card, `--section-gap` between
  them (`SettingsSection`, a named region)
- Card: `--surface-card`, radius `--card-radius` (16px), no outline (Increase
  Contrast adds one). Rows sit inside `--card-pad-x`
- **One row pattern everywhere:** icon in a `--icon-box` (28px) rounded square
  (`--accent-glass` fill, `--accent-text` icon) → bold name (13px semibold)
  → one grey line (11px, `--text-secondary`) → the control on the right.
  `--row-min-h` (52px), `--row-pad-y` top and bottom, a 0.5px `--separator`
  hairline between rows of the same card (`SettingsRow`, or `Row` +
  `RowLabel icon`)
- Secondary buttons are plain grey (`--surface-control`, hover
  `--surface-control-hover`); the one primary is `--accent-fill`

### 5.1c Icons

Lucide (`lucide-react`), one family, **16px, stroke 1.75**, always through
`Icon` / `IconSquare` in `src/ui/icons.tsx` (Lucide's own defaults are 24 at
2). Icons sit beside words and are hidden from VoiceOver; a control that is
only an icon is an `IconButton` with a required `label`. Use them on every
sidebar row, every settings row, and buttons whose action is clear.

### 5.1d Switch and the appearance picker

- Switch: `--switch-w` × `--switch-h` (44 × 24), knob `--switch-knob` (20),
  on = `--accent`, off = `--surface-control-hover`, the knob's side says the
  state as well as the fill
- Appearance: three picture tiles (System split corner to corner, Light,
  Dark), each a drawing of a window in that look from the fixed
  `--preview-*` tokens. A native radio group; the chosen tile gets an accent
  outline, an `--accent-glass` fill and its name in `--accent-text`. A grey
  note under it, then the "See-through glass" switch row

### 5.2 Floating toolbar

One `.glass-group` capsule, floating over the content with `--shadow-floating`.

- Container radius `--radius-capsule`, padding `--toolbar-pad` (4px)
- Item radius is computed concentric, item height 32px
- Icons only, 16px, from one family with matching stroke weight
- Every icon button carries an `aria-label`. Icon-only controls without a name
  are invisible to VoiceOver.
- The primary action is the only tinted item

### 5.3 Notes pane

The content layer. **Opaque**, because this is where sustained reading and
writing happen, and translucency under body text is fatiguing.

- Fill `--surface-content`, padding `--notes-pad` (24px)
- Text 14px at 1.6 line height, as wide as the page column (the cards' width)
- Scrolls under the floating toolbar with `.scroll-edge --top-only`
- Autosaves. Per SPEC, markdown on disk is the source of truth, so the pane owns
  no state the file does not have.

### 5.4 Live transcript panel

Glass, collapsible, floating over the notes pane at the right edge.

- Width `--transcript-w` (320px), radius `--radius-panel`
- Collapsed it is a 32px capsule showing the speaker count and a waveform
- Expanded it uses `.glass` with `.scroll-edge`
- Rows: timestamp, then the speaker (a coloured dot and the word, shown only
  when the speaker changes), then text
- **Volatile versus final text is the important distinction.** Text still being
  revised by the recognizer renders at `--transcript-volatile` (tertiary). It
  never persists to disk, per SPEC. Finalized text steps up to
  `--transcript-final`. That contrast step is the user's signal that a line has
  settled.
- Speaker colors pair with initials, so the transcript survives colorblindness
  and grayscale printing
- Expanding morphs the glass via `.morph-transcript` rather than cross-fading

### 5.5 Menu bar popover

The app's most-used surface, since recording starts here.

- `.glass` with radius `--radius-panel`, min width 240px
- Item height 24px, radius computed concentric from `--popover-pad`
- Record button is 44px, solid `--status-recording`, never translucent
- Shows current state in one line: idle, or elapsed time while recording
- Full keyboard support. Arrow keys move, Escape dismisses, Return activates.

### 5.6 Recording indicator

Solid, never glass, `z-index: var(--z-recording)`. Nothing overlaps it. It pulses
between full and 55% opacity on a 2 second cycle, and the pulse stops entirely
under Reduce Motion while the indicator stays visible.

State is never carried by the pulse alone. An elapsed timer accompanies it.

---

## 6. Motion

| Token | Duration | Used for |
|---|---|---|
| `--dur-instant` | 80ms | Press feedback |
| `--dur-fast` | 140ms | Hover, tint change |
| `--dur-normal` | 220ms | Popover in, panel expand |
| `--dur-slow` | 320ms | Sheets, glass morph |

Rules that matter more than the numbers:

- **Never animate `backdrop-filter`.** Animate opacity, transform, and
  background-color. Blur transitions force a full recomposite every frame and
  will show up as dropped frames while Whisper is transcribing.
- Exits run at roughly 60 to 70% of the entering duration.
- Every animation is interruptible. A click during a transition takes effect now.
- Motion must express cause and effect. The transcript panel grows from its
  collapsed capsule, so the user sees where it came from.

---

## 7. Accessibility floor

These are settings real users have switched on, not hypotheticals. Glass is
decoration and the interface has to survive losing all of it.

- **Reduce Transparency** replaces every blur with an opaque surface. Handled in
  `glass.css` and the token blocks in `tokens.css`. Test it. On macOS: System
  Settings, Accessibility, Display. WebKit may not report the media query; the
  native material turns solid there by itself, and the in-app glass switch
  does the same for the app's own surfaces.
- **Glass off** (Settings → Appearance, `appearance.glass: false`) sets
  `data-glass="off"`: the sidebar and the record prompt turn solid.
- **Increase Contrast** turns rims into solid borders and collapses secondary
  text into primary.
- **Reduce Motion** removes travel and the recording pulse, keeps every state.
- Body text holds 4.5:1 against its **composed** backdrop. Glass makes this hard
  to eyeball, because the effective background depends on what is behind the
  window. Measure against the worst realistic case, which is a bright backdrop in
  light mode, and a mid-gray one in dark.
- Focus rings are 2px at 2px offset, outside the glass, and are never removed.
- Icon-only controls always carry an accessible name.
- Full keyboard operation, including starting and stopping a recording.

---

## 8. Performance budget

This matters more here than in a typical app. meet-ai runs beside Zoom while
capturing system audio and running Whisper on the Metal backend. UI compositing
competes with transcription for GPU time.

- **Cap concurrent glass surfaces at three.** Each `backdrop-filter` is a
  separate GPU pass over its region.
- `.glass--lensed` doubles that cost. Reserve it for one large surface at most,
  and drop it first if frames suffer.
- Never animate blur radius or the lens mask.
- Virtualize the transcript past 50 rows. A three hour meeting produces thousands.
- Keep per-frame work under 16ms. If glass and transcription cannot both fit,
  glass yields. Recording accuracy is the product; the material is not.

---

## 9. Anti-patterns

| Do not | Why |
|---|---|
| Glass on glass | Blurs compound into mud and double the GPU cost |
| Body text on clear glass | Contrast is not guaranteed against an unknown backdrop |
| Tinting more than one control | The tint stops meaning "primary action" |
| Hand-typed inner radii | Corners stop being concentric as soon as padding changes |
| Shipping a webfont | Reads as a website in a window frame |
| 16px base text | macOS convention is 13px, and 16 looks inflated |
| Emoji as icons | Font-dependent, unthemeable, inconsistent across systems |
| Animating `backdrop-filter` | Full recomposite per frame, drops frames during transcription |
| Color-only speaker labels | Fails colorblind users; pair color with the speaker's name |
| A pulsing dot as the only recording signal | Reduce Motion kills the pulse; pair it with a timer |
| Removing the focus ring | Breaks keyboard operation outright |

---

## 10. Before shipping a surface

- [ ] Only one glass layer deep anywhere on screen
- [ ] Concentric radii computed, not typed
- [ ] Tested with Reduce Transparency on
- [ ] Tested with Increase Contrast on
- [ ] Tested with Reduce Motion on
- [ ] Tested in light and dark, against both a bright and a dark backdrop window
- [ ] Body text measures 4.5:1 against the composed backdrop
      (`node design-system/meet-ai/contrast.mjs` passes)
- [ ] Icons through `Icon` / `IconSquare`, 16px, stroke 1.75
- [ ] Rows follow the one row pattern; groups are cards
- [ ] Every icon-only control has an accessible name
- [ ] Full keyboard path, including record and stop
- [ ] Focus rings visible everywhere and never clipped by a glass rim
- [ ] Frame time under 16ms with transcription running
- [ ] Traffic lights unobstructed, drag region works, buttons still click
