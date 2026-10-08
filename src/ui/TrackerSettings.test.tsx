import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { TrackerServer } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { TrackerSettings } from "./TrackerSettings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { trackerSettings, trackerServers, setTracker, sendTestTicket } = ipc;

const SERVERS: TrackerServer[] = [
  { name: "claude.ai Linear", status: "connected" },
  { name: "jira", status: "needs_auth" },
  { name: "github", status: "failed" },
];

function serverName(): HTMLInputElement {
  return screen.getByLabelText("Server name") as HTMLInputElement;
}

/** The tracker field; the section shares its name, so look for the select. */
function trackerField(): HTMLSelectElement {
  return screen.getByLabelText("Tracker", { selector: "select" }) as HTMLSelectElement;
}

describe("TrackerSettings", () => {
  test("shows the saved tracker, server and agent", async () => {
    render(<TrackerSettings />);

    await waitFor(() => expect(serverName().value).toBe("claude.ai Linear"));
    expect(trackerField().value).toBe("linear");
    expect(screen.getByText("Claude Code")).toBeTruthy();
    expect(screen.getByText(/claude mcp add --scope user/)).toBeTruthy();
    expect(screen.getByText(/never stores tracker tokens/)).toBeTruthy();
    // Nothing changed yet, so there is nothing to save.
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });

  test("lists the agent's servers with their status in plain words", async () => {
    trackerServers.mockResolvedValue(SERVERS);
    render(<TrackerSettings />);

    expect(
      await screen.findByRole("option", { name: "claude.ai Linear (Connected)" }),
    ).toBeTruthy();
    expect(screen.getByRole("option", { name: "jira (Needs sign-in)" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "github (Could not connect)" })).toBeTruthy();
  });

  test("says it is checking while the slow server list loads, and the rest still works", async () => {
    let finish: (servers: TrackerServer[]) => void = () => {};
    trackerServers.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    render(<TrackerSettings />);

    expect(screen.getByText("Checking your agent's servers…")).toBeTruthy();
    await waitFor(() => expect(serverName().value).toBe("claude.ai Linear"));

    await act(async () => finish(SERVERS));
    expect(screen.queryByText("Checking your agent's servers…")).toBeNull();
  });

  test("picking a server and a tracker saves both", async () => {
    trackerServers.mockResolvedValue(SERVERS);
    render(<TrackerSettings />);

    const server = await screen.findByLabelText("Server");
    fireEvent.change(trackerField(), { target: { value: "jira" } });
    fireEvent.change(server, { target: { value: "jira" } });
    expect(serverName().value).toBe("jira");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(setTracker).toHaveBeenCalledWith("jira", "jira"));
    expect(await screen.findByText("Saved")).toBeTruthy();
  });

  test("a name typed by hand saves too", async () => {
    render(<TrackerSettings />);
    await waitFor(() => expect(serverName().value).toBe("claude.ai Linear"));

    fireEvent.change(serverName(), { target: { value: "  my-linear  " } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(setTracker).toHaveBeenCalledWith("linear", "my-linear"));
  });

  test("a failed save shows the message", async () => {
    setTracker.mockRejectedValueOnce({ domain: "app", kind: "x", message: "Could not save." });
    render(<TrackerSettings />);
    await waitFor(() => expect(serverName().value).toBe("claude.ai Linear"));

    fireEvent.change(serverName(), { target: { value: "other" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save.");
  });

  test("with no agent it says sending needs one", async () => {
    trackerSettings.mockResolvedValue({
      tracker: "github",
      trackerMcp: "github",
      harness: "none",
    });
    render(<TrackerSettings />);

    expect(await screen.findByText(/Sending tickets needs Claude Code or Codex/)).toBeTruthy();
  });

  test("Check again asks the agent again, and a failure shows in its spot", async () => {
    trackerServers.mockRejectedValueOnce({
      domain: "app",
      kind: "agent-not-installed",
      message: "Claude Code is not installed.",
    });
    render(<TrackerSettings />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Claude Code is not installed.");

    trackerServers.mockResolvedValueOnce(SERVERS);
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));

    expect(await screen.findByRole("option", { name: "jira (Needs sign-in)" })).toBeTruthy();
    expect(trackerServers).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("explains how sending works, and is the tracker anchor", async () => {
    const { container } = render(<TrackerSettings />);
    expect(await screen.findByText(/meet-ai doesn't sign in to your tracker itself/)).toBeTruthy();
    expect(
      screen.getByText("Connect your tracker in your agent first, e.g. claude.ai Linear."),
    ).toBeTruthy();
    expect(container.querySelector("section#tracker")).not.toBeNull();
  });

  test("Send a test ticket checks the values on screen and says what it found", async () => {
    render(<TrackerSettings />);
    await waitFor(() => expect(serverName().value).toBe("claude.ai Linear"));
    fireEvent.change(serverName(), { target: { value: "my linear" } });
    fireEvent.click(screen.getByRole("button", { name: "Send a test ticket" }));
    await waitFor(() => expect(sendTestTicket).toHaveBeenCalledWith("linear", "my linear"));
    expect(
      await screen.findByText("Claude Code reached Linear. New tickets will go to Engineering."),
    ).toBeTruthy();
  });

  test("a failed test ticket shows the message in plain words", async () => {
    sendTestTicket.mockRejectedValueOnce({
      domain: "app",
      kind: "sync-unreachable",
      message: "Your agent couldn't reach Linear.",
    });
    render(<TrackerSettings />);
    await waitFor(() => expect(serverName().value).toBe("claude.ai Linear"));
    fireEvent.click(screen.getByRole("button", { name: "Send a test ticket" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Your agent couldn't reach Linear.");
  });
});
