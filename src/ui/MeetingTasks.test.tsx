import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { MEETINGS_CHANGED_EVENT } from "@/ipc/client";
import type { TicketSummary } from "@/ipc/types";
import { ticketSummary } from "@/test/fixtures";
import { emit, ipc, listening } from "@/test/ipcMock";
import { MeetingTasks } from "./MeetingTasks";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { meetingTasks, approveTask, approveAllTasks, discardTask } = ipc;

const MEETING = "2026-09-30-1015-meeting";

function task(id: string, over: Partial<TicketSummary> = {}): TicketSummary {
  return ticketSummary({
    id,
    title: `Task ${id}`,
    meeting: MEETING,
    body: `Why ${id} matters.`,
    suggested: true,
    ...over,
  });
}

function renderTasks() {
  return render(
    <MemoryRouter>
      <MeetingTasks meetingId={MEETING} />
    </MemoryRouter>,
  );
}

function row(title: string): HTMLElement {
  return screen.getByText(title).closest("li") as HTMLElement;
}

describe("MeetingTasks", () => {
  test("renders nothing when the meeting has no tasks", async () => {
    meetingTasks.mockResolvedValue([]);
    const { container } = renderTasks();
    await waitFor(() => expect(meetingTasks).toHaveBeenCalledWith(MEETING));
    expect(container.innerHTML).toBe("");
  });

  test("suggested tasks show title, description, owner, due, Approve and Discard", async () => {
    meetingTasks.mockResolvedValue([
      task("TICK-1", { owner: "Sam", due: "Friday" }),
      task("TICK-2"),
    ]);
    renderTasks();

    expect(await screen.findByRole("heading", { name: "Suggested tasks" })).toBeTruthy();
    expect(
      screen.getByText(
        "Your agent found these in the meeting. Approve the ones you want to keep: they move to Tickets. Discard the rest.",
      ),
    ).toBeTruthy();
    const first = row("Task TICK-1");
    expect(within(first).getByText("Why TICK-1 matters.")).toBeTruthy();
    expect(within(first).getByText("Owner: Sam")).toBeTruthy();
    expect(within(first).getByText("Due: Friday")).toBeTruthy();
    expect(within(first).getByRole("button", { name: "Approve TICK-1" })).toBeTruthy();
    expect(within(first).getByRole("button", { name: "Discard TICK-1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Approve all" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Sync/ })).toBeNull();
  });

  test("the description shows two lines, and pressing it shows the rest", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1")]);
    renderTasks();
    const text = await screen.findByText("Why TICK-1 matters.");
    const toggle = text.closest("button") as HTMLElement;
    expect(text.className).toContain("line-clamp-2");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    expect(text.className).not.toContain("line-clamp-2");
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
  });

  test("an empty description gets a No description badge", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1", { body: "  " })]);
    renderTasks();
    expect(await screen.findByText("No description")).toBeTruthy();
  });

  test("Approve keeps the task, marked Approved with a link to Tickets and no buttons", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1"), task("TICK-2")]);
    approveTask.mockResolvedValue(task("TICK-1", { suggested: false }));
    renderTasks();

    fireEvent.click(await screen.findByRole("button", { name: "Approve TICK-1" }));
    await waitFor(() => expect(approveTask).toHaveBeenCalledWith(MEETING, "TICK-1"));
    const approved = row("Task TICK-1");
    expect(await within(approved).findByText("Approved")).toBeTruthy();
    expect(
      within(approved).getByRole("link", { name: "See in Tickets" }).getAttribute("href"),
    ).toBe("/tickets");
    expect(within(approved).queryByRole("button")?.textContent ?? null).not.toMatch(
      /Approve|Discard/,
    );
    expect(within(row("Task TICK-2")).getByRole("button", { name: "Approve TICK-2" })).toBeTruthy();
  });

  test("a sent approved task shows its tracker key", async () => {
    meetingTasks.mockResolvedValue([
      task("TICK-1", { suggested: false, syncedTo: "linear", externalId: "ENG-42" }),
    ]);
    renderTasks();
    expect(await screen.findByText("Approved · In Linear: ENG-42")).toBeTruthy();
  });

  test("Discard drops the row", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1"), task("TICK-2")]);
    renderTasks();
    fireEvent.click(await screen.findByRole("button", { name: "Discard TICK-2" }));
    await waitFor(() => expect(discardTask).toHaveBeenCalledWith(MEETING, "TICK-2"));
    await waitFor(() => expect(screen.queryByText("Task TICK-2")).toBeNull());
    expect(screen.getByText("Task TICK-1")).toBeTruthy();
  });

  test("Approve all approves the rest, then hides and says all are handled", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1", { suggested: false }), task("TICK-2")]);
    approveAllTasks.mockResolvedValue([
      task("TICK-1", { suggested: false }),
      task("TICK-2", { suggested: false }),
    ]);
    renderTasks();
    fireEvent.click(await screen.findByRole("button", { name: "Approve all" }));
    await waitFor(() => expect(approveAllTasks).toHaveBeenCalledWith(MEETING));
    expect(
      await screen.findByText("All tasks handled. Approved ones are in Tickets."),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Approve all" })).toBeNull();
    expect(screen.getAllByText("Approved")).toHaveLength(2);
  });

  test("a failed approve shows its error under the row and keeps the buttons", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1")]);
    approveTask.mockRejectedValue({ domain: "app", kind: "x", message: "Disk is full." });
    renderTasks();
    fireEvent.click(await screen.findByRole("button", { name: "Approve TICK-1" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("Disk is full.");
    const title = screen.getByText("Task TICK-1");
    expect(title.parentElement?.contains(alert)).toBe(false);
    expect(screen.getByRole("button", { name: "Approve TICK-1" })).toBeTruthy();
  });

  test("reloads when the meeting's files change", async () => {
    meetingTasks.mockResolvedValue([task("TICK-1")]);
    renderTasks();
    await screen.findByText("Task TICK-1");
    await waitFor(() => expect(listening(MEETINGS_CHANGED_EVENT)).toBe(true));
    meetingTasks.mockResolvedValue([task("TICK-1"), task("TICK-2")]);
    act(() => emit(MEETINGS_CHANGED_EVENT, { paths: [] }));
    expect(await screen.findByText("Task TICK-2")).toBeTruthy();
  });
});
