import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { RecordingStatus } from "@/ipc/types";
import { RecordControl } from "./RecordControl";

/** TUR-146: the main window's Pause button beside Stop. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const shortcut = vi.hoisted(() => ({ available: true }));
vi.mock("@/ipc/shortcut", () => ({
  recordShortcutAvailable: () => Promise.resolve(shortcut.available),
}));

const RECORDING: RecordingStatus = {
  phase: "recording",
  meetingId: "m1",
  startedAtMs: 0,
  error: null,
  pause: { pausedAtMs: null, pausedTotalMs: 0 },
};

function renderControl(status: RecordingStatus, busy = false) {
  const onToggle = vi.fn();
  const onTogglePause = vi.fn();
  render(
    <RecordControl
      status={status}
      permission={null}
      busy={busy}
      onToggle={onToggle}
      onTogglePause={onTogglePause}
    />,
  );
  return { onToggle, onTogglePause };
}

describe("RecordControl pause", () => {
  test("idle has no Pause button", () => {
    renderControl({ ...RECORDING, phase: "idle", meetingId: null, startedAtMs: null });
    expect(screen.queryByRole("button", { name: /pause|resume/i })).toBeNull();
  });

  test("recording has Pause, and it calls the pause toggle, not Stop", () => {
    const { onToggle, onTogglePause } = renderControl(RECORDING);
    fireEvent.click(screen.getByRole("button", { name: "Pause recording" }));
    expect(onTogglePause).toHaveBeenCalledOnce();
    expect(onToggle).not.toHaveBeenCalled();
  });

  test("paused shows Resume, a still dot and the frozen timer with the word Paused", () => {
    renderControl({ ...RECORDING, pause: { pausedAtMs: 75_000, pausedTotalMs: 5_000 } });
    expect(screen.getByRole("button", { name: "Resume recording" })).toBeInTheDocument();
    expect(screen.getByText("00:01:10 Paused")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /^Stop recording\. 00:01:10 so far, paused\.$/ }),
    ).toBeInTheDocument();
    expect(document.querySelector(".record__dot--live")).toBeNull();
  });

  test("Pause waits while a request is in flight", () => {
    renderControl(RECORDING, true);
    expect(screen.getByRole("button", { name: "Pause recording" })).toBeDisabled();
  });
});

describe("RecordControl shortcut (TUR-169)", () => {
  const IDLE: RecordingStatus = { ...RECORDING, phase: "idle", meetingId: null, startedAtMs: null };

  test("a registered shortcut is advertised on the button", async () => {
    shortcut.available = true;
    await act(async () => {
      renderControl(IDLE);
    });
    expect(screen.getByRole("button", { name: "Start recording" }).title).toBe("⌘⇧R");
    expect(screen.queryByText(/unavailable/)).toBeNull();
  });

  test("a shortcut another app owns is shown as unavailable", async () => {
    shortcut.available = false;
    await act(async () => {
      renderControl(IDLE);
    });
    const button = screen.getByRole("button", { name: "Start recording" });
    expect(button.title).toBe(
      "⌘⇧R is unavailable: another app is using it. Use the Record button instead.",
    );
    expect(screen.getByText(/is unavailable: another app is using it/)).toBeInTheDocument();
  });
});
