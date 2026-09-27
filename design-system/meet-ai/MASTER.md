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
| `MASTER.md` | Rules, component specs, and what not to do |

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

Line length in the notes pane is capped at `--notes-measure` (68ch). Transcript
lines are short by nature and need no cap.

Use tabular figures for timestamps so the transcript does not shift as the clock
ticks: `font-variant-numeric: tabular-nums`.

---

## 5. Component specs

### 5.1 Sidebar — meeting list

Native vibrancy carries this surface. It gets **no** `backdrop-filter` of its
own, because the window material is already behind it.

- Width `--sidebar-w` (228px), resizable between 180 and 320px
- Row height `--sidebar-row-h` (28px), radius `--radius-chip`
- Hover `--sidebar-row-hover`, selected `--sidebar-row-active`
- Selection is marked by fill **and** a leading accent bar, never by color alone
- Row content: meeting title (13px, truncates), relative time (11px, tertiary)
- Recording meetings show the pulsing dot, not a red title

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
- Text 14px at 1.6 line height, measure capped at 68ch
- Scrolls under the floating toolbar with `.scroll-edge --top-only`
- Autosaves. Per SPEC, markdown on disk is the source of truth, so the pane owns
  no state the file does not have.

### 5.4 Live transcript panel

Glass, collapsible, floating over the notes pane at the right edge.

- Width `--transcript-w` (320px), radius `--radius-panel`
- Collapsed it is a 32px capsule showing the speaker count and a waveform
- Expanded it uses `.glass` with `.scroll-edge`
- Rows: speaker initial in a colored circle, then text, then timestamp
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
  `glass.css`. Test it. On macOS: System Settings, Accessibility, Display.
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
| Color-only speaker labels | Fails colorblind users; pair color with an initial |
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
- [ ] Every icon-only control has an accessible name
- [ ] Full keyboard path, including record and stop
- [ ] Focus rings visible everywhere and never clipped by a glass rim
- [ ] Frame time under 16ms with transcription running
- [ ] Traffic lights unobstructed, drag region works, buttons still click
