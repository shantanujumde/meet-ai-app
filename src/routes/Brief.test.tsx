import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { describe, expect, test, vi } from "vitest";
import type { MeetingBrief } from "@/ipc/types";
import { briefPath } from "@/lib/routes";
import { ipc } from "@/test/ipcMock";
import { Brief } from "./Brief";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { meetingBrief } = ipc;

function fullBrief(over: Partial<MeetingBrief> = {}): MeetingBrief {
  return {
    title: "Platform Standup",
    previous: {
      id: "2026-09-08-1430-standup",
      title: "Platform Standup",
      date: "2026-09-08T14:30:00+05:30",
      summary: "Sessions still live in memory.",
      decisions: "- Move sessions to Redis.",
      openTickets: [
        { id: "TICK-0001", title: "Redis session store", status: "open" },
        { id: "TICK-0002", title: "Load test login", status: "in_progress" },
      ],
    },
    commits: {
      repo: "~/apps/api",
      since: "2026-09-08T14:30:00+05:30",
      commits: [
        { hash: "abc1234", subject: "Add Redis sessions" },
        { hash: "def5678", subject: "Load test the login path" },
      ],
    },
    ...over,
  };
}

function renderBrief(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/brief" element={<Brief />} />
        <Route path="/meetings/:id" element={<p>meeting page</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Brief", () => {
  test("shows last time's summary, decisions, open tickets and the commits since", async () => {
    meetingBrief.mockResolvedValue(fullBrief());
    renderBrief(briefPath("Platform Standup"));

    expect(await screen.findByText("Sessions still live in memory.")).toBeTruthy();
    expect(meetingBrief).toHaveBeenCalledWith("Platform Standup");
    expect(screen.getByRole("heading", { name: "Before Platform Standup" })).toBeTruthy();
    expect(screen.getByText("- Move sessions to Redis.")).toBeTruthy();
    expect(screen.getByText("Redis session store")).toBeTruthy();
    expect(screen.getByText("In progress")).toBeTruthy();
    expect(screen.getByText("~/apps/api")).toBeTruthy();
    expect(screen.getByText("abc1234")).toBeTruthy();
    expect(screen.getByText("Load test the login path")).toBeTruthy();
  });

  test("a title with spaces and symbols survives the link", async () => {
    meetingBrief.mockResolvedValue(fullBrief({ title: "R&D sync #3" }));
    renderBrief(briefPath("R&D sync #3"));
    expect(await screen.findByText("Sessions still live in memory.")).toBeTruthy();
    expect(meetingBrief).toHaveBeenCalledWith("R&D sync #3");
  });

  test("says plainly when there is no earlier meeting", async () => {
    meetingBrief.mockResolvedValue({ title: "Kickoff", previous: null, commits: null });
    renderBrief(briefPath("Kickoff"));
    expect(await screen.findByRole("heading", { name: "Nothing from last time" })).toBeTruthy();
    expect(screen.getByText(/no earlier meeting called “Kickoff”/)).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Commits since then" })).toBeNull();
  });

  test("leaves the commits out when there is no repo", async () => {
    meetingBrief.mockResolvedValue(fullBrief({ commits: null }));
    renderBrief(briefPath("Platform Standup"));
    expect(await screen.findByText("Sessions still live in memory.")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Commits since then" })).toBeNull();
  });

  test("blank sections and no open tickets say so", async () => {
    const base = fullBrief();
    meetingBrief.mockResolvedValue(
      fullBrief({
        previous: base.previous && {
          ...base.previous,
          summary: null,
          decisions: null,
          openTickets: [],
        },
      }),
    );
    renderBrief(briefPath("Platform Standup"));
    expect(await screen.findByText("No summary was written.")).toBeTruthy();
    expect(screen.getByText("No decisions were written.")).toBeTruthy();
    expect(screen.getByText("No tickets left open.")).toBeTruthy();
  });

  test("Open last meeting goes to that meeting", async () => {
    meetingBrief.mockResolvedValue(fullBrief());
    renderBrief(briefPath("Platform Standup"));
    (await screen.findByRole("button", { name: "Open last meeting" })).click();
    expect(await screen.findByText("meeting page")).toBeTruthy();
  });

  test("no title asks nothing of Rust", async () => {
    renderBrief("/brief");
    expect(await screen.findByRole("heading", { name: "No meeting picked" })).toBeTruthy();
    expect(meetingBrief).not.toHaveBeenCalled();
  });

  test("a failed read shows the error", async () => {
    meetingBrief.mockRejectedValue({
      domain: "app",
      kind: "search-index",
      message: "Could not read the index.",
    });
    renderBrief(briefPath("Platform Standup"));
    expect(await screen.findByText("Could not read the index.")).toBeTruthy();
  });
});
