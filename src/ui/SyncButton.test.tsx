import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { ticketSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { SendError, SendStatus, sendLabel, trackerName } from "./SyncButton";
import type { SendState } from "./useTicketSync";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { dismissUnsavedSync, openSyncedIssue } = ipc;

const SENT = ticketSummary({
  syncedTo: "linear",
  externalId: "ENG-42",
  externalUrl: "https://linear.app/team/issue/ENG-42",
});

function failed(kind: string, message = "It broke."): SendState {
  return { kind: "failed", error: { domain: "app", kind, message } };
}

describe("sendLabel", () => {
  test("each state has its words", () => {
    const ticket = ticketSummary();
    expect(sendLabel({ kind: "not-sent" }, "linear", ticket)).toBe("Not sent");
    expect(sendLabel({ kind: "sending" }, "linear", ticket)).toBe("Sending to Linear…");
    expect(sendLabel(failed("x"), "jira", ticket)).toBe("Couldn't send to Jira");
    expect(sendLabel({ kind: "sent" }, "linear", SENT)).toBe("In Linear: ENG-42");
  });

  test("names each tracker, with a fallback for one it does not know", () => {
    expect(trackerName("linear")).toBe("Linear");
    expect(trackerName("github")).toBe("GitHub");
    expect(trackerName("other")).toBe("other");
  });
});

describe("SendStatus", () => {
  test("a sent ticket opens its issue through Rust", async () => {
    render(<SendStatus ticket={SENT} state={{ kind: "sent" }} tracker="linear" />);
    fireEvent.click(screen.getByRole("button", { name: "In Linear: ENG-42, open the issue" }));
    await waitFor(() => expect(openSyncedIssue).toHaveBeenCalledWith("TUR-7", null));
  });

  test("a failed open shows the message", async () => {
    openSyncedIssue.mockRejectedValueOnce({ domain: "app", kind: "x", message: "No browser." });
    render(<SendStatus ticket={SENT} state={{ kind: "sent" }} tracker="linear" />);
    fireEvent.click(screen.getByRole("button", { name: "In Linear: ENG-42, open the issue" }));
    expect(await screen.findByText("No browser.")).toBeTruthy();
  });
});

function renderError(state: SendState, onRetry = vi.fn()) {
  render(
    <MemoryRouter>
      <SendError ticket={ticketSummary()} state={state} onRetry={onRetry} />
    </MemoryRouter>,
  );
  return onRetry;
}

describe("SendError", () => {
  test("nothing unless the send failed", () => {
    const { container } = render(
      <MemoryRouter>
        <SendError ticket={ticketSummary()} state={{ kind: "sending" }} onRetry={vi.fn()} />
      </MemoryRouter>,
    );
    expect(container.innerHTML).toBe("");
  });

  test("unreachable and refused offer Open Tracker settings next to Retry", () => {
    for (const kind of ["sync-unreachable", "sync-refused"]) {
      const onRetry = renderError(failed(kind));
      expect(screen.getByRole("button", { name: "Open Tracker settings" })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Retry sending TUR-7" }));
      expect(onRetry).toHaveBeenCalled();
      document.body.innerHTML = "";
    }
  });

  test("a stopped send offers only Retry", () => {
    renderError(failed("agent-timed-out", "Sending stopped before it finished. Press Retry."));
    expect(screen.getByText("Sending stopped before it finished. Press Retry.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Open Tracker settings" })).toBeNull();
  });

  test("a kept issue offers Dismiss, which forgets it and retries", async () => {
    const onRetry = renderError(failed("sync-not-saved"));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss the unsaved issue of TUR-7" }));
    await waitFor(() => expect(dismissUnsavedSync).toHaveBeenCalledWith("TUR-7", null));
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
  });
});
