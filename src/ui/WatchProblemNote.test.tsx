import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { MEETINGS_WATCH_PROBLEM_EVENT } from "@/ipc/client";
import { emit, ipc } from "@/test/ipcMock";
import { WATCH_PROBLEM_TEXT, WatchProblemNote } from "./WatchProblemNote";

/** TUR-134: say so when the meetings folder cannot be watched. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

describe("WatchProblemNote", () => {
  test("shows the problem pulled when the page opens", async () => {
    ipc.meetingsWatchProblem.mockResolvedValue("too many folders");
    render(<WatchProblemNote />);
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain(WATCH_PROBLEM_TEXT),
    );
    expect(screen.getByText("too many folders")).toBeTruthy();
  });

  test("shows the problem from the event and hides on null", async () => {
    render(<WatchProblemNote />);
    await waitFor(() => expect(ipc.meetingsWatchProblem).toHaveBeenCalled());
    expect(screen.queryByRole("status")).toBeNull();
    act(() => emit(MEETINGS_WATCH_PROBLEM_EVENT, { message: "limit hit" }));
    expect(screen.getByText("limit hit")).toBeTruthy();
    act(() => emit(MEETINGS_WATCH_PROBLEM_EVENT, { message: null }));
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("the text has no em dash", () => {
    expect(WATCH_PROBLEM_TEXT).not.toContain("\u2014");
  });
});
