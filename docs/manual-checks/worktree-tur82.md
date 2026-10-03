# Manual checks: TUR-82

Coming back to a meeting from Settings showed a blank pane: every route renders
into the shell's one `<main className="content">` pane, so its `scrollTop`
survived the route change. Fix: `useResetScrollOnRouteChange`
(`src/ui/useResetScrollOnRouteChange.ts`), used once by `src/ui/Shell.tsx`,
puts the pane at the top in a layout effect whenever the pathname changes
(the pathname carries the meeting id, so meeting → meeting resets too).

The test (`src/ui/Shell.scroll.test.tsx`) runs in jsdom, which does no layout,
so it gives the pane a fake geometry with a writable `scrollTop`. It fails
without the fix and passes with it. It cannot prove what WebKit does, so these
need the real app.

## Run by hand

1. **Settings → meeting.** Open a meeting, open Settings, scroll it to the
   bottom, click the meeting in the sidebar.
   Expect: the meeting's title is visible at once, pane at the top.
   Why skipped: needs the running app.

2. **Same, while recording.** Start a recording, let the live transcript run
   for a minute, do check 1.
   Expect: title visible at once; the live transcript box still follows new
   lines inside its own box, and the outer pane does not move as lines land.
   Why skipped: needs the running app, a mic and system-audio permission.

3. **Every route.** Scroll down on a meeting, then go to Settings, Tickets,
   another meeting and back, scrolling each one down first.
   Expect: each route opens at the top.
   Why skipped: needs the running app.

## Not done here

- `LiveTranscript` already scrolls only its own box
  (`scroller.scrollTop = scroller.scrollHeight`), and there is no
  `scrollIntoView` or `autoFocus` in `src/`, so neither needed a change.
- The "remember each meeting's position" nice-to-have is skipped: restoring
  an old offset would conflict with "title visible at once" in check 1.
- `src/routes/Review.tsx` is not touched.
