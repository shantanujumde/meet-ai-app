import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { QUIT_CONFIRM_EVENT } from "@/ipc/client";
import type { RecordingStatus } from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";
import { emit, ipc } from "@/test/ipcMock";
import { QuitPrompt } from "./QuitPrompt";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const RECORDING: RecordingStatus = {
  phase: "recording",
  meetingId: "2026-10-03-1800-meeting",
  startedAtMs: 1,
  error: null,
};

function ask() {
  act(() => emit(QUIT_CONFIRM_EVENT, null));
}

function dialog() {
  return screen.queryByRole("alertdialog", { name: "Stop recording and quit?" });
}

describe("QuitPrompt", () => {
  test("shows nothing until Rust holds a quit", () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    expect(dialog()).toBeNull();
  });

  test("asks, with Cancel focused so Return is never the destructive answer", () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    expect(dialog()).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  test("Stop and quit tells Rust to go ahead", async () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    const stop = screen.getByRole("button", { name: "Stop and quit" });
    // Destructive: the danger fill, not the accent.
    expect(stop.className).toContain("bg-danger");
    expect(stop.className).not.toContain("bg-accent ");
    await act(async () => {
      fireEvent.click(stop);
    });
    expect(ipc.confirmQuit).toHaveBeenCalledTimes(1);
  });

  test("Cancel keeps meet-ai running", () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(dialog()).toBeNull();
    expect(ipc.confirmQuit).not.toHaveBeenCalled();
  });

  test("Escape is Cancel", () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    fireEvent.keyDown(screen.getByRole("alertdialog"), { key: "Escape" });
    expect(dialog()).toBeNull();
    expect(ipc.confirmQuit).not.toHaveBeenCalled();
  });

  test("Tab stays inside the dialog", () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const stop = screen.getByRole("button", { name: "Stop and quit" });
    stop.focus();
    fireEvent.keyDown(stop, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(cancel, { key: "Tab", shiftKey: true });
    expect(stop).toHaveFocus();
  });

  test("a recording that ends some other way closes the question", () => {
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    act(() => useRecordingStore.setState({ status: { ...RECORDING, phase: "idle" } }));
    expect(dialog()).toBeNull();
  });

  test("a quit that fails leaves the choice on screen", async () => {
    ipc.confirmQuit.mockRejectedValueOnce({ kind: "no-backend", message: "no" });
    useRecordingStore.setState({ status: RECORDING });
    render(<QuitPrompt />);
    ask();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Stop and quit" }));
    });
    expect(screen.getByRole("button", { name: "Stop and quit" })).not.toBeDisabled();
  });
});
