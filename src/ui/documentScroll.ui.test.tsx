import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import type { LiveLine } from "@/ipc/types";
import { lockDocumentScroll } from "@/lib/documentScroll";
import { EMPTY_LIVE, type LiveTranscript as LiveState } from "@/state/transcript";
import { LiveTranscript } from "./LiveTranscript";

/**
 * The document itself never scrolls, whatever the UI does (TUR-15).
 *
 * After a laptop restart the window opened with the whole page scrolled up:
 * the content sat off the top and blank material filled the bottom. Only the
 * panes are meant to scroll, so the page's own scroll position is pinned at 0.
 *
 * jsdom does no layout, has no `document.scrollingElement`, and `focus()`
 * never scrolls anything. So these tests stand in for WebKit by hand: they
 * give the document a scrolling element, move it the way the browser would
 * (set `scrollTop`, fire `scroll` on the window), and check it comes back to
 * 0 while the pane keeps its own position and focus stays where it was put.
 */

const line = (seq: number, speaker: LiveLine["speaker"], text: string): LiveLine => ({
  seq,
  speaker,
  start_sec: seq * 5,
  text,
});

function stateWith(partial: Partial<LiveState>): LiveState {
  return {
    ...EMPTY_LIVE,
    status: { state: "running", engine: "apple-speech", detail: null },
    ...partial,
  };
}

/** Give the scroller a fake geometry, with a writable scrollTop. */
function fakeGeometry(
  element: HTMLElement,
  geometry: { scrollHeight: number; clientHeight: number },
) {
  let top = 0;
  Object.defineProperty(element, "scrollHeight", {
    configurable: true,
    get: () => geometry.scrollHeight,
  });
  Object.defineProperty(element, "clientHeight", {
    configurable: true,
    get: () => geometry.clientHeight,
  });
  Object.defineProperty(element, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      top = Math.min(value, geometry.scrollHeight - geometry.clientHeight);
    },
  });
}

/**
 * Track the document's scroll position, recording every write to it, so a
 * test can tell "never moved" apart from "moved and was put back".
 */
function watchDocumentScroll() {
  const writes: number[] = [];
  let top = 0;
  Object.defineProperty(document.documentElement, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      writes.push(value);
      top = value;
    },
  });
  return writes;
}

/** What WebKit does when it scrolls the page: move it, then say so. */
function browserScrollsDocument(to: number) {
  document.documentElement.scrollTop = to;
  window.dispatchEvent(new Event("scroll"));
}

const scrollingElement = () => document.scrollingElement as Element;

let unlock: () => void = () => {};

beforeEach(() => {
  // A real WebKit page always has one; jsdom does not.
  Object.defineProperty(document, "scrollingElement", {
    configurable: true,
    get: () => document.documentElement,
  });
});

afterEach(() => {
  unlock();
  unlock = () => {};
  Reflect.deleteProperty(document, "scrollingElement");
  Reflect.deleteProperty(document.documentElement, "scrollTop");
});

describe("the document stays at the top", () => {
  test("the live transcript following a new line scrolls its pane, not the page", () => {
    const writes = watchDocumentScroll();
    unlock = lockDocumentScroll(window);

    const geometry = { scrollHeight: 1000, clientHeight: 400 };
    const { rerender } = render(
      <LiveTranscript live={stateWith({ finals: [line(1, "you", "a")] })} />,
    );
    const pane = screen.getByRole("log").parentElement as HTMLElement;
    fakeGeometry(pane, geometry);
    pane.scrollTop = 600;
    fireEvent.scroll(pane);

    geometry.scrollHeight = 1100;
    rerender(
      <LiveTranscript
        live={stateWith({ finals: [line(1, "you", "a"), line(2, "others", "b")] })}
      />,
    );

    expect(pane.scrollTop).toBe(700);
    expect(scrollingElement().scrollTop).toBe(0);
    // The pane's scroll never touched the page's position at all.
    expect(writes.filter((value) => value !== 0)).toEqual([]);
  });

  test("the browser scrolling the page while the transcript runs is put back", () => {
    watchDocumentScroll();
    unlock = lockDocumentScroll(window);

    const geometry = { scrollHeight: 1000, clientHeight: 400 };
    render(<LiveTranscript live={stateWith({ finals: [line(1, "you", "a")] })} />);
    const pane = screen.getByRole("log").parentElement as HTMLElement;
    fakeGeometry(pane, geometry);
    pane.scrollTop = 600;

    browserScrollsDocument(500);

    expect(scrollingElement().scrollTop).toBe(0);
    // Putting the page back leaves the pane where the reader had it.
    expect(pane.scrollTop).toBe(600);
  });

  test("focus moving to something low on the page does not leave the page scrolled", () => {
    watchDocumentScroll();
    unlock = lockDocumentScroll(window);

    render(
      <>
        <LiveTranscript live={stateWith({ finals: [line(1, "you", "a")] })} />
        <textarea aria-label="Notes" />
      </>,
    );
    const notes = screen.getByRole("textbox", { name: "Notes" });

    notes.focus();
    // WebKit scrolls the page to bring the focused field into view.
    browserScrollsDocument(800);

    expect(scrollingElement().scrollTop).toBe(0);
    // Putting the page back does not take focus away.
    expect(document.activeElement).toBe(notes);
  });
});
