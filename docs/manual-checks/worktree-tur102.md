# TUR-102: whole-app restyle (glass sidebar, brand-tinted surfaces, one row pattern)

Contrast is checked headless by `node design-system/meet-ai/contrast.mjs`
(also `src/test/contrast.test.ts` in CI): 792 text/surface pairs over 8
environments, 0 below the minimum (4.5:1 for text, 3:1 for icons and the
switch). See-through surfaces are composed over an assumed backdrop: the
sidebar over mid-grey `#808080`, the popup over black and over white. The
real backdrop behind the native window material is not knowable headless:
measure it.

| Environment | Lowest text pair |
|---|---|
| Light | 4.55:1 (`--accent-text` on the sidebar) |
| Dark | 4.64:1 (`--text-tertiary` on a hovered button on a card) |
| Light, Increase Contrast | 4.99:1 |
| Dark, Increase Contrast | 5.14:1 |
| Light, Reduce Transparency / glass off | 4.88:1 |
| Dark, Reduce Transparency / glass off | 4.64:1 |

TUR-100's popup surface: primary 14.1 to 15.1 (light), 13.1 to 14.7 (dark);
secondary 7.58 to 7.86 (light), 8.18 to 8.92 (dark).

## 1. Look at every screen in light and dark (skipped: needs the running app)

- Run: a signed build (`just bundle-signed`), open meet-ai, finish setup.
- Walk through: Meetings (empty and with meetings), a meeting's Review page
  (finished, and while recording), Tickets, Brief (from the Today pane),
  Settings top to bottom, Setup again (Settings, Show setup again), the
  record prompt (open a meeting app) and the quit prompt (⌘Q while recording).
- Do it with Settings → Appearance on Light, then Dark, then System with the
  OS in each mode.
- Expected: calm, roomy pages; big bold title; rounded cards a step off a
  faintly tinted background (warm-violet near-white in light, violet-tinted
  ink in dark, not neutral grey); every settings row is icon square, bold
  name, grey line, control on the right; all icons the same size and weight;
  the current sidebar row is a solid blue pill with white text and icon;
  switches are the bigger 44 × 24 ones; secondary buttons plain grey.
- Why skipped: no display or signed build in this worktree.

## 2. Glass sidebar over a busy backdrop (skipped: needs the running app)

- Run: as 1, with a bright photo wallpaper, then a dark one, then a busy
  window (a web page, a code editor) beside and behind meet-ai.
- Expected: the sidebar lets a little of the window material through, in a
  tone a little different from the content; all sidebar text (headings,
  meeting dates, "● Recording", the blue icons) reads clearly. The title bar
  and the content column stay opaque. The window corners, shadow and the
  dimming when the window loses focus look as before.
- Measure: if any sidebar text looks faint, take a screenshot and measure the
  composed colours; the token check assumes the backdrop is never darker than
  mid-grey in light mode or lighter than mid-grey in dark mode.
- Windows: the sidebar is solid (Mica only shows in the native title bar, as
  TUR-57 found). Linux: solid. Check that nothing is see-through there.
- Why skipped: the native material and the desktop behind it cannot be seen
  headless.

## 3. Light / Dark / System and the glass switch persist (skipped: needs the running app)

- Run: Settings → Appearance, pick Dark with the OS in light mode.
- Expected: the whole window turns dark at once, including the native
  title bar, traffic lights and the material behind the sidebar
  (`AppHandle::set_theme`). `~/Meetings/.app/config.jsonc` gets
  `"appearance": { "theme": "dark", "glass": true }`, comments and other
  keys kept. Quit and reopen: still dark, with no light flash after the
  window shows.
- Pick System, switch the OS between light and dark: meet-ai follows live.
- Turn See-through glass off: the sidebar turns solid at once, and the record
  prompt popup (Windows, Linux) opens solid. `"glass": false` is saved.
- Edit `config.jsonc` by hand to `"theme": "sepia"`: the app opens with the
  defaults and logs a warning; saving from Settings then shows the error
  instead of overwriting the file.
- Note: a record prompt popup window that is already open when the choice
  changes keeps its old look until it next opens.
- Why skipped: needs the running app, a display and the real config file.

## 4. Increase Contrast and Reduce Transparency (skipped: needs OS settings)

- Run: System Settings → Accessibility → Display: turn on Increase
  Contrast, then (separately) Reduce Transparency. On Windows: Settings →
  Personalisation → Colours → Transparency effects off; High contrast on.
- Expected, Increase Contrast: cards get solid edges, grey lines turn
  primary-coloured, separators darken, the sidebar is solid, every
  `contrast-more:` border shows.
- Expected, Reduce Transparency: the sidebar is solid. WebKit may not report
  `prefers-reduced-transparency`; the native material then turns solid by
  itself, so the sidebar is solid either way. Measure it: confirm the sidebar
  shows no backdrop.
- Why skipped: changing OS accessibility settings is not allowed here.

## 5. Keyboard and VoiceOver (skipped: needs the running app)

- Run: Tab through the title bar, sidebar and Settings → Appearance with
  VoiceOver on.
- Expected: Back and Forward are announced by name and are disabled when
  there is nowhere to go; the sidebar group headings say expanded or
  collapsed and fold with Space; the appearance tiles are one radio group
  ("Colours") moved with the arrow keys, each tile showing a focus ring;
  icons are not announced; every focus ring is visible.
- Why skipped: needs the running app and VoiceOver.

## 6. Frame time (skipped: needs the running app and a recording)

- Run: record a meeting with live transcription while scrolling the Review
  page and the sidebar.
- Expected: no dropped frames; the sidebar adds no `backdrop-filter` pass of
  its own (it is a flat alpha fill over the native material).
- Why skipped: needs real audio and the running app.

## Things not verified here

- `specimen.html` was not opened in a browser (no browser may be opened in
  this worktree). It links `tokens.css` and `glass.css` from its own folder.
- The Tailwind output was compiled headless with `@tailwindcss/node` to check
  that the new arbitrary classes (`rounded-(--card-radius)`,
  `outline-(color:--focus-ring)`, the switch's `calc()` classes, `bg-danger/12`)
  produce CSS; how they render was not seen.
