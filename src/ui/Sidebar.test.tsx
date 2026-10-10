import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import type { MeetingSummary, RecordingStatus } from "@/ipc/types";
import { meetingSummary } from "@/test/fixtures";
import { Sidebar } from "./Sidebar";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const IDLE: RecordingStatus = {
  phase: "idle",
  meetingId: null,
  startedAtMs: null,
  pause: { pausedAtMs: null, pausedTotalMs: 0 },
  error: null,
};

const STANDUP = meetingSummary({ id: "a", title: "Platform standup", time: "09:00" });
const REVIEW = meetingSummary({ id: "b", title: "Design review", time: "14:00" });

function show(
  meetings: MeetingSummary[],
  { at = "/meetings", selectedId = undefined as string | undefined } = {},
) {
  render(
    <MemoryRouter initialEntries={[at]}>
      <Sidebar
        list={{ root: "/Users/test/Meetings", rootExists: true, meetings }}
        loading={false}
        recording={IDLE}
        selectedId={selectedId}
      />
    </MemoryRouter>,
  );
  return screen.getByRole("navigation", { name: "Sidebar" });
}

describe("Sidebar (TUR-102)", () => {
  test("marks the current page, not by colour alone", () => {
    const nav = show([STANDUP], { at: "/tickets" });
    expect(within(nav).getByRole("link", { name: "Tickets" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(within(nav).getByRole("link", { name: "Meetings" })).not.toHaveAttribute("aria-current");
    expect(within(nav).getByRole("link", { name: "Settings" })).toBeInTheDocument();
  });

  test("the open meeting is the current row", () => {
    const nav = show([STANDUP, REVIEW], { at: "/meetings/b", selectedId: "b" });
    const row = within(nav).getByText("Design review").closest("button");
    expect(row).toHaveAttribute("aria-current", "true");
    expect(within(nav).getByText("Platform standup").closest("button")).toHaveAttribute(
      "aria-current",
      "false",
    );
  });

  test("the find box narrows the meeting list by title", async () => {
    const nav = show([STANDUP, REVIEW]);
    await act(async () => {
      fireEvent.change(within(nav).getByRole("searchbox", { name: "Find a meeting" }), {
        target: { value: "design" },
      });
    });
    expect(within(nav).getByText("Design review")).toBeInTheDocument();
    expect(within(nav).queryByText("Platform standup")).not.toBeInTheDocument();

    await act(async () => {
      fireEvent.change(within(nav).getByRole("searchbox", { name: "Find a meeting" }), {
        target: { value: "nothing like it" },
      });
    });
    expect(within(nav).getByText("No meeting called that.")).toBeInTheDocument();
  });

  test("a group heading folds its rows shut and opens them again", async () => {
    const nav = show([STANDUP]);
    const heading = within(nav).getByRole("button", { name: /Recent meetings/ });
    expect(heading).toHaveAttribute("aria-expanded", "true");
    await act(async () => {
      fireEvent.click(heading);
    });
    expect(heading).toHaveAttribute("aria-expanded", "false");
    expect(within(nav).queryByText("Platform standup")).not.toBeInTheDocument();
    await act(async () => {
      fireEvent.click(heading);
    });
    expect(within(nav).getByText("Platform standup")).toBeInTheDocument();
  });
});
