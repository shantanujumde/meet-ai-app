import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { RECORDING_STATE_EVENT, TRANSCRIPT_UPDATE_EVENT } from "@/ipc/client";
import type { RecordingStatus } from "@/ipc/types";
import { EMPTY_LIVE } from "@/state/transcript";
import { emit, ipc } from "@/test/ipcMock";
import { emptyText, latestLines, Overlay } from "./Overlay";

/** TUR-146: the small always-on-top recording card. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const MEETING = "2026-10-08-1000-meeting";

const RECORDING: RecordingStatus = {
  phase: "recording",
  meetingId: MEETING,
  startedAtMs: Date.now() - 65_000,
  error: null,
  pause: { pausedAtMs: null, pausedTotalMs: 0 },
};

const PAUSED: RecordingStatus = {
  ...RECORDING,
  startedAtMs: 1_000,
  pause: { pausedAtMs: 1_000 + 3_725_000, pausedTotalMs: 0 },
};

function line(seq: number, text: string, kind: "final" | "volatile" = "final") {
  act(() => {
    emit(TRANSCRIPT_UPDATE_EVENT, { kind, seq, speaker: "you", start_sec: seq, text });
  });
}

async function renderRecording(status: RecordingStatus = RECORDING) {
  ipc.recordingStatus.mockResolvedValue(status);
  render(<Overlay />);
  await waitFor(() => expect(screen.getByRole("button", { name: "Stop recording" })).toBeEnabled());
}

describe("Overlay", () => {
  test("shows the timer, Pause and Stop while recording", async () => {
    await renderRecording();
    expect(screen.getByRole("timer", { name: /^Recording for 01:0\d$/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pause recording" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop recording" })).toBeInTheDocument();
    expect(screen.getByTestId("overlay-dot")).toHaveClass("record__dot--live");
    expect(screen.getByText(emptyText(false))).toBeInTheDocument();
  });

  test("shows only the last two lines, the newest guess included", async () => {
    await renderRecording();
    line(1, "First thing.");
    line(2, "Second thing.");
    line(3, "Third thing.");
    line(4, "And now", "volatile");
    const shown = screen.getAllByTestId("overlay-line").map((span) => span.textContent);
    expect(shown).toEqual(["Third thing.", "And now"]);
  });

  test("Pause and Resume go to Rust, and a paused card freezes and says so", async () => {
    ipc.pauseRecording.mockResolvedValue(PAUSED);
    await renderRecording();
    line(1, "Said before the pause.");
    fireEvent.click(screen.getByRole("button", { name: "Pause recording" }));
    await waitFor(() => expect(ipc.pauseRecording).toHaveBeenCalledOnce());

    await screen.findByRole("button", { name: "Resume recording" });
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.getByRole("timer", { name: "Paused at 1:02:05" })).toBeInTheDocument();
    expect(screen.getByTestId("overlay-dot")).not.toHaveClass("record__dot--live");
    expect(screen.queryByTestId("overlay-line")).toBeNull();
    expect(screen.getByText(emptyText(true))).toBeInTheDocument();

    ipc.resumeRecording.mockResolvedValue(RECORDING);
    fireEvent.click(screen.getByRole("button", { name: "Resume recording" }));
    await waitFor(() => expect(ipc.resumeRecording).toHaveBeenCalledOnce());
  });

  test("Stop is the same stop as the main window's", async () => {
    await renderRecording();
    fireEvent.click(screen.getByRole("button", { name: "Stop recording" }));
    await waitFor(() => expect(ipc.stopRecording).toHaveBeenCalledOnce());
  });

  test("clicking the text brings the main window forward", async () => {
    await renderRecording();
    fireEvent.click(screen.getByRole("button", { name: /^Open meet-ai/ }));
    await waitFor(() => expect(ipc.overlayShowMain).toHaveBeenCalledOnce());
  });

  test("the buttons wait while the recording stops", async () => {
    await renderRecording();
    act(() => emit(RECORDING_STATE_EVENT, { ...RECORDING, phase: "stopping" }));
    expect(screen.getByRole("button", { name: "Stop recording" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Pause recording" })).toBeDisabled();
  });

  test("every icon button has a name", async () => {
    await renderRecording();
    for (const button of screen.getAllByRole("button")) {
      expect(button).toHaveAccessibleName();
    }
  });

  test("the card is the drag handle and the text is not", async () => {
    await renderRecording();
    expect(screen.getByRole("region", { name: "meet-ai recording" })).toHaveAttribute(
      "data-tauri-drag-region",
    );
    expect(screen.getByRole("button", { name: /^Open meet-ai/ })).not.toHaveAttribute(
      "data-tauri-drag-region",
    );
  });
});

describe("latestLines", () => {
  test("merges settled lines and guesses by seq", () => {
    const live = {
      ...EMPTY_LIVE,
      finals: [1, 2, 5].map((seq) => ({
        seq,
        speaker: "you" as const,
        start_sec: seq,
        text: `${seq}`,
      })),
      volatile: {
        you: null,
        others: { seq: 4, speaker: "others" as const, start_sec: 4, text: "4" },
      },
    };
    expect(latestLines(live).map((l) => l.text)).toEqual(["4", "5"]);
    expect(latestLines(EMPTY_LIVE)).toEqual([]);
  });

  test("the empty text has no em dash", () => {
    for (const paused of [true, false]) expect(emptyText(paused)).not.toContain("\u2014");
  });
});
