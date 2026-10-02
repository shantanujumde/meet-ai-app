import { afterEach, describe, expect, test } from "vitest";
import { lockDocumentScroll, resetDocumentScroll } from "./documentScroll";

/**
 * TUR-15: the page itself never scrolls, only the panes inside the shell.
 *
 * jsdom does no layout, so nothing here scrolls by itself: each test moves the
 * document by hand, the way WebKit would, and checks the lock moves it back.
 * The `overflow: hidden` in index.css cannot be tested here (vitest loads CSS
 * as an empty string); docs/manual-checks covers it in the real window.
 */

const root = document.documentElement;
let undo: (() => void) | null = null;

afterEach(() => {
  undo?.();
  undo = null;
  root.scrollTop = 0;
  root.scrollLeft = 0;
});

describe("resetDocumentScroll", () => {
  test("puts a scrolled document back at the top left", () => {
    root.scrollTop = 480;
    root.scrollLeft = 12;
    resetDocumentScroll();
    expect(root.scrollTop).toBe(0);
    expect(root.scrollLeft).toBe(0);
  });
});

describe("lockDocumentScroll", () => {
  test("undoes a scroll position the window came back with", () => {
    // What WebKit restoring the last session looks like by the time the
    // bundle runs: the document already scrolled before anything mounted.
    root.scrollTop = 600;
    undo = lockDocumentScroll();
    expect(root.scrollTop).toBe(0);
  });

  test("asks the browser not to restore a page position", () => {
    // jsdom has no `scrollRestoration`; WebKit has, and defaults it to "auto".
    Object.defineProperty(window.history, "scrollRestoration", {
      configurable: true,
      writable: true,
      value: "auto",
    });
    undo = lockDocumentScroll();
    expect(window.history.scrollRestoration).toBe("manual");
    Reflect.deleteProperty(window.history, "scrollRestoration");
  });

  test("snaps the document back whenever it scrolls or the window resizes", () => {
    undo = lockDocumentScroll();

    root.scrollTop = 300;
    window.dispatchEvent(new Event("scroll"));
    expect(root.scrollTop).toBe(0);

    root.scrollTop = 120;
    window.dispatchEvent(new Event("resize"));
    expect(root.scrollTop).toBe(0);
  });

  test("leaves a pane's own scroll alone", () => {
    undo = lockDocumentScroll();
    const pane = document.createElement("div");
    document.body.append(pane);
    pane.scrollTop = 250;
    // A pane's scroll event does not bubble, so the lock never hears it.
    pane.dispatchEvent(new Event("scroll"));
    expect(pane.scrollTop).toBe(250);
    pane.remove();
  });

  test("stops once undone", () => {
    lockDocumentScroll()();
    root.scrollTop = 300;
    window.dispatchEvent(new Event("scroll"));
    expect(root.scrollTop).toBe(300);
  });
});
