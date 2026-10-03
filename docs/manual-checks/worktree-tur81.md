# tur81 manual checks (meeting header, TUR-81)

The layout is covered by Vitest (`src/routes/Review.test.tsx`, "Review's header"
and "Review's notes switch"). What a headless run cannot show is how it looks.
All of these need the running, signed app, which an agent run may not start.

1. **Before/after screenshot.** Open a finished meeting. Expected: the title, one
   muted line under it ("Today · 22:36 · 50 min"), and two small icon buttons
   (folder, copy) on the right of the title row. No folder path printed. Take a
   screenshot for the PR.
2. **While recording.** Start a recording and open its meeting. Expected: the meta
   line starts with a red dot and "Recording", and its length counts up each
   minute. The dot stops pulsing with Reduce Motion on; the word stays.
3. **Tooltip.** Hover the folder button. Expected: the full folder path as the
   tooltip. Click: Finder opens the folder.
4. **Copy folder path.** Click the copy button, paste in TextEdit. Expected: the
   folder path; the button shows a check mark for about 2 s.
5. **Notes switch.** Expected under "Meeting notes", in a card like Settings: the
   label and its helper line on the left, the switch on the right. Works while
   recording and after; switched off then on for a finished meeting offers
   "Make notes now".
6. **Appearance.** Light and dark, Increase Contrast on: meta text turns primary,
   icons follow the text colour, focus rings visible on both icon buttons.

Why skipped: needs the running app (`just dev` / a signed build), which this run
is not allowed to start.
