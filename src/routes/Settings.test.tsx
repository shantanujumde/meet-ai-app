import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { settingsPath } from "@/lib/routes";
import { ipc } from "@/test/ipcMock";
import { BLUETOOTH_MIC_LABEL } from "@/ui/BluetoothMicSetting";
import { DOCK_SETTING_LABEL } from "@/ui/DockSetting";
import { Settings } from "./Settings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const RETENTION_TEXT = /^Audio is kept for 7 days\./;

async function show(path = "/settings") {
  render(
    <MemoryRouter initialEntries={[path]}>
      <Settings />
    </MemoryRouter>,
  );
  await act(async () => {});
}

describe("Settings: Audio section", () => {
  test("holds the audio retention row and the Bluetooth mic row", async () => {
    await show();
    const audio = screen.getByRole("region", { name: "Audio" });
    expect(await within(audio).findByText(RETENTION_TEXT)).toBeTruthy();
    expect(within(audio).getByRole("switch", { name: BLUETOOTH_MIC_LABEL })).toBeTruthy();
  });

  test("Files has neither row", async () => {
    await show();
    const files = screen.getByRole("region", { name: "Files" });
    expect(within(files).queryByText(RETENTION_TEXT)).toBeNull();
    expect(within(files).queryByRole("switch", { name: BLUETOOTH_MIC_LABEL })).toBeNull();
  });

  test("Audio comes right after Speech", async () => {
    await show();
    const titles = screen
      .getAllByRole("heading", { level: 2 })
      .map((heading) => heading.textContent);
    const speech = titles.indexOf("Speech");
    expect(speech).toBeGreaterThanOrEqual(0);
    expect(titles[speech + 1]).toBe("Audio");
  });
});

describe("Settings: a section link (TUR-113)", () => {
  test("?section=tracker scrolls the Tracker section into view", async () => {
    const scrolled: Element[] = [];
    const before = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = function (this: Element) {
      scrolled.push(this);
    };
    try {
      await show(settingsPath("tracker"));
      expect(scrolled.map((each) => each.id)).toEqual(["tracker"]);
      expect(screen.getByRole("region", { name: "Tracker" }).id).toBe("tracker");
    } finally {
      Element.prototype.scrollIntoView = before;
    }
  });
});

describe("Settings: fewer backend calls (TUR-171)", () => {
  test("the saved values come from one snapshot, not a read per card", async () => {
    await show();
    await waitFor(() => expect(ipc.detectAgents).toHaveBeenCalled());
    expect(ipc.settingsSnapshot).toHaveBeenCalledTimes(1);
    for (const read of [
      ipc.appSettings,
      ipc.menuBarCountdown,
      ipc.audioRetentionDays,
      ipc.builtinMicWithBluetooth,
      ipc.showRecordingOverlay,
      ipc.agentChoice,
      ipc.notesAutoRun,
      ipc.notificationSettings,
      ipc.calendarSources,
      ipc.trackerSettings,
      ipc.configProblem,
    ]) {
      expect(read).not.toHaveBeenCalled();
    }
  });

  test("the cards show what the snapshot read", async () => {
    const saved = await ipc.settingsSnapshot();
    if (!saved) throw new Error("the mock always answers");
    ipc.settingsSnapshot.mockResolvedValueOnce({
      ...saved,
      showInDockWhenClosed: true,
      audioRetention: { state: "running", days: 30 },
      detectionProblem: "detection.min_attendees 20 is outside 1 to 10; using 10",
    });
    ipc.settingsSnapshot.mockClear();
    await show();
    expect(await screen.findByText(/^Audio is kept for 30 days\./)).toBeTruthy();
    expect(screen.getByRole("switch", { name: DOCK_SETTING_LABEL })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(await screen.findByText(/detection\.min_attendees 20/)).toBeTruthy();
    expect(ipc.configProblem).not.toHaveBeenCalled();
  });

  test("a snapshot that fails falls back to each card's own read", async () => {
    ipc.settingsSnapshot.mockRejectedValueOnce({ domain: "app", kind: "io", message: "No." });
    await show();
    expect(await screen.findByText(RETENTION_TEXT)).toBeTruthy();
    expect(ipc.audioRetentionDays).toHaveBeenCalledTimes(1);
    expect(ipc.appSettings).toHaveBeenCalledTimes(1);
  });

  test("the engine probe runs once: the saved engine's error comes with the choices", async () => {
    await show();
    await waitFor(() => expect(ipc.engineChoices).toHaveBeenCalledTimes(1));
    expect(ipc.engineSelection).not.toHaveBeenCalled();
  });

  test("a second visit runs no CLI check again; Check again does", async () => {
    const first = await showAndUnmount();
    expect(ipc.detectAgents).toHaveBeenCalledTimes(1);
    expect(ipc.trackerServers).toHaveBeenCalledTimes(1);
    first();

    await show();
    await act(async () => {});
    expect(ipc.detectAgents).toHaveBeenCalledTimes(1);
    expect(ipc.trackerServers).toHaveBeenCalledTimes(1);
    // A fresh snapshot each visit, so a hand edit shows.
    expect(ipc.settingsSnapshot).toHaveBeenCalledTimes(2);

    const tracker = screen.getByRole("region", { name: "Tracker" });
    fireEvent.click(within(tracker).getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(ipc.trackerServers).toHaveBeenCalledTimes(2));
  });
});

/** Render Settings, wait for its checks, and hand back the unmount. */
async function showAndUnmount(): Promise<() => void> {
  const { unmount } = render(
    <MemoryRouter initialEntries={["/settings"]}>
      <Settings />
    </MemoryRouter>,
  );
  await waitFor(() => expect(ipc.trackerServers).toHaveBeenCalled());
  await waitFor(() => expect(ipc.detectAgents).toHaveBeenCalled());
  await act(async () => {});
  return unmount;
}
