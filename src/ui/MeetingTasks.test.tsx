import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { MEETINGS_CHANGED_EVENT } from "@/ipc/client";
import type { TicketSummary } from "@/ipc/types";
import { ticketSummary } from "@/test/fixtures";
import { emit, ipc, listening } from "@/test/ipcMock";
import { MeetingTasks } from "./MeetingTasks";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { meetingTasks, syncTask, cancelSync, dismissUnsavedSync, trackerSettings } = ipc;

const MEETING = "2026-09-30-1015-meeting";

function task(id: string, over: Partial<TicketSummary> = {}): TicketSummary {
  return ticketSummary({ id, title: `Task ${id}`, meeting: MEETING, ...over });
}

function synced(ticket: TicketSummary): TicketSummary {
  return {
    ...ticket,
    syncedTo: "linear",
    externalId: `ENG-${ticket.id}`,
    externalUrl: `https://linear.app/issue/${ticket.id}`,
  };
}

/** Each sync waits until the test settles it, in the order they were started. */
function heldSyncs() {
  const runs: { id: string; resolve: () => void; reject: (error: unknown) => void }[] = [];
  syncTask.mockImplementation(
    (id) =>
      new Promise<TicketSummary>((resolve, reject) => {
        runs.push({ id, resolve: () => resolve(synced(task(id))), reject });
      }),
  );
  return runs;
}

describe("MeetingTasks", () => {
  test("renders nothing when the meeting has no tasks", async () => {
    meetingTasks.mockResolvedValue([]);
    const { container } = render(<MeetingTasks meetingId={MEETING} />);
    await waitFor(() => expect(meetingTasks).toHaveBeenCalledWith(MEETING));
    expect(container.innerHTML).toBe("");
  });

  test("lists the meeting's tasks with id, title and Sync", async () => {
    meetingTasks.mockResolvedValue([task("TUR-1"), synced(task("TUR-2"))]);
    render(<MeetingTasks meetingId={MEETING} />);

    expect(await screen.findByText("Task TUR-1")).toBeTruthy();
    expect(screen.getByText("TUR-1")).toBeTruthy();
    expect(await screen.findByRole("button", { name: "Sync TUR-1" })).toBeTruthy();
    expect(screen.getByText("ENG-TUR-2")).toBeTruthy();
    expect(screen.getByText("1 not synced yet")).toBeTruthy();
  });

  test("Sync all runs the unsynced tasks one at a time, updating each row", async () => {
    meetingTasks.mockResolvedValue([task("TUR-1"), synced(task("TUR-2")), task("TUR-3")]);
    const runs = heldSyncs();
    render(<MeetingTasks meetingId={MEETING} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync all" }));

    await waitFor(() => expect(runs).toHaveLength(1));
    expect(syncTask).toHaveBeenLastCalledWith("TUR-1", MEETING);
    expect((screen.getByRole("button", { name: "Sync all" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect((screen.getByRole("button", { name: "Sync TUR-3" }) as HTMLButtonElement).disabled).toBe(
      true,
    );

    await act(async () => runs[0]?.resolve());
    expect(await screen.findByText("ENG-TUR-1")).toBeTruthy();
    await waitFor(() => expect(runs).toHaveLength(2));
    expect(syncTask).toHaveBeenLastCalledWith("TUR-3", MEETING);

    await act(async () => runs[1]?.resolve());
    expect(await screen.findByText("ENG-TUR-3")).toBeTruthy();
    expect(syncTask).toHaveBeenCalledTimes(2);
    expect(screen.getByText("All synced")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Sync all" })).toBeNull();
  });

  test("a failure stays on its row and Sync all carries on with the next", async () => {
    meetingTasks.mockResolvedValue([task("TUR-1"), task("TUR-2")]);
    const runs = heldSyncs();
    render(<MeetingTasks meetingId={MEETING} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync all" }));
    await waitFor(() => expect(runs).toHaveLength(1));
    await act(async () =>
      runs[0]?.reject({ domain: "app", kind: "agent-failed", message: "The agent gave up." }),
    );

    await waitFor(() => expect(runs).toHaveLength(2));
    const firstRow = screen.getByText("Task TUR-1").closest("li") as HTMLElement;
    expect(within(firstRow).getByRole("alert")).toHaveTextContent("The agent gave up.");
    expect(within(firstRow).getByRole("button", { name: "Retry sync of TUR-1" })).toBeTruthy();

    await act(async () => runs[1]?.resolve());
    expect(await screen.findByText("ENG-TUR-2")).toBeTruthy();
  });

  test("a long sync error sits under the row, outside the controls column (TUR-112)", async () => {
    const message =
      'The agent finished but did not create an issue in Linear. Check that "claude.ai Linear" is connected and signed in.';
    meetingTasks.mockResolvedValue([task("TICK-0001", { title: "Build human session closure" })]);
    syncTask.mockRejectedValueOnce({ domain: "app", kind: "agent-failed", message });
    render(<MeetingTasks meetingId={MEETING} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync TICK-0001" }));
    const row = screen.getByText("Build human session closure").closest("li") as HTMLElement;
    const alert = await within(row).findByRole("alert");
    expect(alert).toHaveTextContent(message);

    const retry = within(row).getByRole("button", { name: "Retry sync of TICK-0001" });
    const rowLine = row.firstElementChild as HTMLElement;
    expect(rowLine.contains(retry)).toBe(true);
    // The error is a sibling of the row line, not inside the right-hand column.
    expect(rowLine.contains(alert)).toBe(false);
    expect(alert.closest("li > *")).not.toBe(rowLine);
    // The title column takes the free width; the controls do not shrink.
    expect(screen.getByText("Build human session closure").closest(".flex-1")).toBeTruthy();
  });

  test("Dismiss forgets a row's unattached issue with this meeting's id", async () => {
    meetingTasks.mockResolvedValue([task("TUR-1", { meeting: null })]);
    syncTask.mockRejectedValueOnce({
      domain: "app",
      kind: "sync-not-attached",
      message:
        "Created in Linear but couldn't attach it to this task: https://linear.app/issue/ENG-1",
    });
    render(<MeetingTasks meetingId={MEETING} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync TUR-1" }));
    const row = screen.getByText("Task TUR-1").closest("li") as HTMLElement;
    expect(await within(row).findByRole("alert")).toHaveTextContent(
      "https://linear.app/issue/ENG-1",
    );

    fireEvent.click(
      within(row).getByRole("button", { name: "Dismiss the unsaved issue of TUR-1" }),
    );
    await waitFor(() => expect(dismissUnsavedSync).toHaveBeenCalledWith("TUR-1", MEETING));
    expect(await within(row).findByRole("button", { name: "Sync TUR-1" })).toBeTruthy();
    expect(within(row).queryByRole("alert")).toBeNull();
  });

  test("Cancel stops Sync all", async () => {
    meetingTasks.mockResolvedValue([task("TUR-1"), task("TUR-2")]);
    const runs = heldSyncs();
    render(<MeetingTasks meetingId={MEETING} />);

    fireEvent.click(await screen.findByRole("button", { name: "Sync all" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel sync of TUR-1" }));
    expect(cancelSync).toHaveBeenCalledWith("TUR-1");

    await act(async () =>
      runs[0]?.reject({ domain: "app", kind: "agent-cancelled", message: "Cancelled." }),
    );

    expect(await screen.findByRole("button", { name: "Sync TUR-1" })).toBeTruthy();
    expect(syncTask).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect((screen.getByRole("button", { name: "Sync all" }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });

  test("with no agent the tasks show, but no Sync buttons", async () => {
    trackerSettings.mockResolvedValue({
      tracker: "linear",
      trackerMcp: "claude.ai Linear",
      harness: "none",
    });
    meetingTasks.mockResolvedValue([task("TUR-1")]);
    render(<MeetingTasks meetingId={MEETING} />);

    expect(await screen.findByText("Task TUR-1")).toBeTruthy();
    await waitFor(() => expect(trackerSettings).toHaveBeenCalled());
    await act(async () => {});
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("reloads when the meeting's files change", async () => {
    meetingTasks.mockResolvedValue([task("TUR-1")]);
    render(<MeetingTasks meetingId={MEETING} />);
    await screen.findByText("Task TUR-1");
    await waitFor(() => expect(listening(MEETINGS_CHANGED_EVENT)).toBe(true));

    meetingTasks.mockResolvedValue([task("TUR-1"), task("TUR-2")]);
    act(() => emit(MEETINGS_CHANGED_EVENT, { paths: [] }));

    expect(await screen.findByText("Task TUR-2")).toBeTruthy();
  });
});
