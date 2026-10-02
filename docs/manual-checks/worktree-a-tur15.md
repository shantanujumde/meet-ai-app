# Manual checks: TUR-15

The window opened scrolled up after a laptop restart. Fix: `html, body, #root`
get `overflow: hidden; overscroll-behavior: none` (`src/index.css`), the shell's
content row is `minmax(0, 1fr)` (`src/app.css`), and `lockDocumentScroll()`
(`src/lib/documentScroll.ts`, called from `src/main.tsx`) puts the document back
at 0 whenever it scrolls or the window resizes, and turns off scroll restoring.

The tests (`src/lib/documentScroll.test.ts`, `src/ui/documentScroll.ui.test.tsx`)
run in jsdom, which does no layout and loads CSS as an empty string. They prove
the lock and that the live transcript scrolls only its own pane. They cannot
prove the CSS or what WebKit does, so these need the real app.

## Run by hand

1. **Restart with "Reopen windows".** System Settings → Desktop & Dock (or the
   restart dialog): "Reopen windows when logging back in" on. Open a long
   meeting (transcript longer than the window), scroll the transcript to the
   bottom, quit with the window open, restart the Mac.
   Expect: meet-ai opens at its normal layout: titlebar, sidebar with
   "Settings" at the bottom, content column from the top. No blank band.
   Why skipped: needs a restart of the Mac and the running app.

2. **Very short window and back.** With a long meeting open, drag the window to
   its smallest height, then back to full height. Do it again while recording,
   with the live transcript following new lines.
   Expect: the layout fills the window each time; nothing is left scrolled.
   At the smallest height the sidebar footer may be cut off, but the page does
   not scroll.
   Why skipped: needs the running app and a real window.

3. **Page cannot be scrolled by hand.** Two-finger scroll and rubber-band over
   the titlebar and the sidebar heading; press Space / Page Down with focus on
   the body (click an empty area first).
   Expect: nothing moves except the pane under the pointer (meeting list,
   content column, live transcript). No bounce of the whole window.
   Why skipped: needs the running app.

4. **Focus low on the page.** In a meeting with a long transcript, Tab through
   until focus reaches the notes box at the bottom.
   Expect: the content column scrolls to show it; the titlebar and sidebar stay
   in place.
   Why skipped: needs the running app.

## Not done here

- The exact trigger on 2026-10-02 is not proven. Candidates: WebKit restoring
  a scroll position when macOS reopened the window, or the window briefly
  restored very short so content overflowed. The fix covers both (no page
  scroll is allowed, and any that happens is undone), so check 1 is the real
  proof.
- `src/routes/Review.tsx` has no auto-scroll; `LiveTranscript` already scrolled
  its own pane (`.live__scroller`), so neither needed a change.
