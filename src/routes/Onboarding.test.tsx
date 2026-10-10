import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { App } from "@/App";

/**
 * The wizard inside the real shell, so the title bar's Back arrow and the
 * `Bootstrap` redirects are the ones a user gets.
 */

vi.mock("@tauri-apps/plugin-os", () => ({ platform: vi.fn(() => "macos") }));
vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

beforeEach(() => {
  window.location.hash = "";
});

/** The router's place in the history; 0 is the first entry the window made. */
function historyIndex(): unknown {
  return (window.history.state as { idx?: unknown } | null)?.idx;
}

test("after finishing setup, Back has no finished steps to walk through", async () => {
  // TUR-165: each step used to push an entry, and each one bounced to Meetings.
  render(<App />);
  await screen.findByRole("heading", { name: /meet-ai records your meetings/i });
  const start = historyIndex();

  const steps: [string, RegExp][] = [
    ["Get started", /let meet-ai hear your mac/i],
    ["Continue", /how meet-ai turns speech into text/i],
    ["Continue", /where your meetings live/i],
    ["Continue", /who writes your notes/i],
  ];
  for (const [button, name] of steps) {
    fireEvent.click(screen.getByRole("button", { name: button }));
    await screen.findByRole("heading", { name });
    expect(historyIndex()).toBe(start);
  }

  // The wizard's own Back moves within the same entry too.
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  await screen.findByRole("heading", { name: /where your meetings live/i });
  expect(historyIndex()).toBe(start);
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByRole("heading", { name: /who writes your notes/i });

  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await screen.findByRole("heading", { name: /no meetings yet/i });
  expect(window.location.hash).toBe("#/meetings");
  expect(historyIndex()).toBe(start);
  await waitFor(() => expect(screen.getByRole("button", { name: "Back" })).toBeDisabled());
});
