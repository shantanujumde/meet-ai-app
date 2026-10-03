import { act, render, screen } from "@testing-library/react";
import { MemoryRouter, type NavigateFunction, Route, Routes, useNavigate } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { Shell } from "./Shell";

/**
 * The content pane starts every route at the top (TUR-82).
 *
 * Every route renders into the shell's one `<main>` pane, so its scroll offset
 * used to survive a route change: scroll Settings to the bottom, open a
 * meeting, and the meeting sat off the top of a blank-looking pane.
 *
 * jsdom does no layout and its `scrollTop` is always 0, so the pane gets a fake
 * geometry with a writable, clamped `scrollTop`, the way WebKit has one.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

/** Give an element a fake geometry, with a writable scrollTop. */
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
      top = Math.max(0, Math.min(value, geometry.scrollHeight - geometry.clientHeight));
    },
  });
}

let go: NavigateFunction = () => {};

/** Hands the test the router's `navigate`, standing in for a sidebar click. */
function Navigator() {
  go = useNavigate();
  return null;
}

/** A route body with its own inner scroller, like the live transcript's box. */
function MeetingPage({ id }: { id: string }) {
  return (
    <>
      <h1>Meeting {id}</h1>
      <div data-testid="inner-scroller" />
    </>
  );
}

function renderShell(at: string) {
  render(
    <MemoryRouter initialEntries={[at]}>
      <Navigator />
      <Routes>
        <Route element={<Shell />}>
          <Route path="/meetings/:id" element={<MeetingPage id="x" />} />
          <Route path="/settings" element={<h1>Settings</h1>} />
          <Route path="/tickets" element={<h1>Tickets</h1>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
  const pane = screen.getByRole("main");
  fakeGeometry(pane, { scrollHeight: 3000, clientHeight: 600 });
  return pane;
}

function navigateTo(path: string) {
  act(() => {
    void go(path);
  });
}

describe("the content pane on a route change", () => {
  test("scrolling Settings to the bottom, then opening a meeting, shows the meeting from the top", () => {
    const pane = renderShell("/settings");
    pane.scrollTop = 2400;
    expect(pane.scrollTop).toBe(2400);

    navigateTo("/meetings/2026-10-03-0900-standup");

    expect(screen.getByRole("heading", { name: "Meeting x" })).toBeInTheDocument();
    expect(pane.scrollTop).toBe(0);
  });

  test("every route change resets it: meeting, settings, tickets, another meeting", () => {
    const pane = renderShell("/meetings/a");
    for (const path of ["/settings", "/tickets", "/meetings/a", "/meetings/b"]) {
      pane.scrollTop = 1200;
      navigateTo(path);
      expect(pane.scrollTop, path).toBe(0);
    }
  });

  test("the reset leaves a route's own inner scroller where it was", () => {
    const pane = renderShell("/settings");
    pane.scrollTop = 1800;

    navigateTo("/meetings/a");
    const inner = screen.getByTestId("inner-scroller");
    fakeGeometry(inner, { scrollHeight: 1000, clientHeight: 400 });
    inner.scrollTop = 600;
    pane.scrollTop = 900;

    // A re-render on the same route (a new live line, a store change) is not
    // a route change, so neither scroll position moves.
    navigateTo("/meetings/a");
    expect(pane.scrollTop).toBe(900);
    expect(inner.scrollTop).toBe(600);
  });
});
