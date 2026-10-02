/**
 * Keep the document itself from ever scrolling (TUR-15).
 *
 * The app shell is a fixed frame: only the panes inside it (the meeting list,
 * the content column, the live transcript) scroll. index.css sets
 * `overflow: hidden` on `html`, `body` and `#root`, which stops the user
 * scrolling the page, but not the browser: `focus()`, `scrollIntoView()` and
 * WebKit restoring an old scroll position when macOS reopens the window after
 * a restart can all still move a `hidden` scroller. Nothing in the app would
 * ever scroll it back, so the window opened showing the bottom of the page
 * with blank material below it.
 *
 * So the document's scroll position is pinned at 0: once at start, and again
 * whenever it moves or the window is resized.
 */

/** Put the document back at the top, if anything moved it. */
export function resetDocumentScroll(doc: Document = document): void {
  // jsdom has no `scrollingElement`; a real WebKit page always does.
  const root = doc.scrollingElement ?? doc.documentElement;
  if (root.scrollTop !== 0) root.scrollTop = 0;
  if (root.scrollLeft !== 0) root.scrollLeft = 0;
}

/**
 * Pin the document's scroll position at 0 from now on. Returns the undo, for
 * tests; the app calls this once and never undoes it.
 */
export function lockDocumentScroll(win: Window = window): () => void {
  const doc = win.document;
  // An old position restored on reload is exactly the bug; there is no page
  // position worth restoring in a single-window app.
  if ("scrollRestoration" in win.history) win.history.scrollRestoration = "manual";

  const reset = () => resetDocumentScroll(doc);
  reset();
  // A pane scrolling fires `scroll` on the pane and does not bubble, so this
  // only hears the document's own scroll.
  win.addEventListener("scroll", reset);
  win.addEventListener("resize", reset);
  return () => {
    win.removeEventListener("scroll", reset);
    win.removeEventListener("resize", reset);
  };
}
