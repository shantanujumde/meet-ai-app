import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { TicketSummary } from "@/ipc/types";
import { ticketSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { openLabel, SyncButton } from "./SyncButton";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { syncTask, cancelSync, openSyncedIssue } = ipc;

const SYNCED = ticketSummary({
  meeting: "2026-09-30-1015-meeting",
  syncedTo: "linear",
  externalId: "ENG-42",
  externalUrl: "https://linear.app/team/issue/ENG-42",
});

/** A sync that stays running until the test settles it. */
function pendingSync() {
  let resolve: (ticket: TicketSummary) => void = () => {};
  let reject: (error: unknown) => void = () => {};
  syncTask.mockImplementation(
    () =>
      new Promise<TicketSummary>((res, rej) => {
        resolve = res;
        reject = rej;
      }),
  );
  return { resolve: (t: TicketSummary) => resolve(t), reject: (e: unknown) => reject(e) };
}

describe("SyncButton", () => {
  test("syncs the ticket with its meeting, shows progress, then reports the update", async () => {
    const run = pendingSync();
    const onSynced = vi.fn();
    const ticket = ticketSummary({ meeting: "2026-09-30-1015-meeting" });
    render(<SyncButton ticket={ticket} onSynced={onSynced} />);

    fireEvent.click(screen.getByRole("button", { name: "Sync TUR-7" }));

    expect(syncTask).toHaveBeenCalledWith("TUR-7", "2026-09-30-1015-meeting");
    expect(await screen.findByText("Syncing…")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("Syncing TUR-7");
    expect(screen.getByRole("button", { name: "Cancel sync of TUR-7" })).toBeTruthy();

    await act(async () => run.resolve(SYNCED));
    expect(onSynced).toHaveBeenCalledWith(SYNCED);
  });

  test("a failed sync shows Rust's message and offers Retry", async () => {
    syncTask.mockRejectedValueOnce({
      domain: "app",
      kind: "sync-not-done",
      message: "The agent finished but did not create an issue.",
    });
    render(<SyncButton ticket={ticketSummary()} onSynced={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Sync TUR-7" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The agent finished but did not create an issue.",
    );
    const retry = screen.getByRole("button", { name: "Retry sync of TUR-7" });
    expect(retry.textContent).toBe("Retry");

    fireEvent.click(retry);
    await waitFor(() => expect(syncTask).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  test("Cancel stops the run and goes back to Sync without an error", async () => {
    const run = pendingSync();
    render(<SyncButton ticket={ticketSummary()} onSynced={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Sync TUR-7" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel sync of TUR-7" }));

    expect(cancelSync).toHaveBeenCalledWith("TUR-7");
    expect(await screen.findByText("Cancelling…")).toBeTruthy();

    await act(async () =>
      run.reject({ domain: "app", kind: "agent-cancelled", message: "Sync was cancelled." }),
    );
    expect(await screen.findByRole("button", { name: "Sync TUR-7" })).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("a synced ticket shows its issue id and opens it through Rust", async () => {
    render(<SyncButton ticket={SYNCED} onSynced={vi.fn()} />);

    expect(screen.getByText("ENG-42")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Sync/ })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Open in Linear" }));
    await waitFor(() =>
      expect(openSyncedIssue).toHaveBeenCalledWith("TUR-7", "2026-09-30-1015-meeting"),
    );
  });

  test("a failed open shows the message", async () => {
    openSyncedIssue.mockRejectedValueOnce({
      domain: "app",
      kind: "unexpected",
      message: "Could not open the browser.",
    });
    render(<SyncButton ticket={SYNCED} onSynced={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Open in Linear" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not open the browser.");
  });

  test("names each tracker, with a fallback for one it does not know", () => {
    expect(openLabel("linear")).toBe("Open in Linear");
    expect(openLabel("jira")).toBe("Open in Jira");
    expect(openLabel("github")).toBe("Open in GitHub");
    expect(openLabel("asana")).toBe("Open issue");
    expect(openLabel(null)).toBe("Open issue");
  });

  test("with no agent there is no Sync button, but a synced issue still opens", () => {
    const { rerender } = render(
      <SyncButton ticket={ticketSummary()} canSync={false} onSynced={vi.fn()} />,
    );
    expect(screen.queryByRole("button")).toBeNull();

    rerender(<SyncButton ticket={SYNCED} canSync={false} onSynced={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Open in Linear" })).toBeTruthy();
  });
});
