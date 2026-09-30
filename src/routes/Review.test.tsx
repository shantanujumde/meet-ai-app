import { act, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { MeetingDetail, RecordingStatus } from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";
import { EMPTY_LIVE, useTranscriptStore } from "@/state/transcript";
import { meetingDetail, transcriptLine } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { Review } from "./Review";

/**
 * The review screen's one live-transcript job (TUR-96): while this meeting is
 * the one recording, the live pane stands in for the file; when the recording
 * ends, the finished `transcript.md` is read again and shown instead.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { readMeeting } = ipc;

const ID = "2026-09-30-1015-meeting";

function detail(lines: MeetingDetail["lines"]): MeetingDetail {
  return meetingDetail({ summary: { id: ID }, lines });
}

function recording(status: Partial<RecordingStatus>) {
  useRecordingStore.setState({
    status: { phase: "idle", meetingId: null, startedAtMs: null, ...status },
  });
}

function renderReview(id = ID) {
  return render(
    <MemoryRouter initialEntries={[`/meetings/${id}`]}>
      <Routes>
        <Route path="/meetings/:id" element={<Review />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("Review while its meeting records", () => {
  beforeEach(() => {
    readMeeting.mockReset();
    useTranscriptStore.setState({
      live: {
        ...EMPTY_LIVE,
        status: { state: "running", engine: "apple-speech", detail: null },
        finals: [{ seq: 0, speaker: "you", start_sec: 4, text: "Said while recording." }],
      },
    });
  });

  test("the live pane replaces the file, then the finished file replaces it at stop", async () => {
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    readMeeting.mockResolvedValueOnce(detail([]));
    renderReview();

    expect(await screen.findByRole("heading", { name: "Live transcript" })).toBeTruthy();
    expect(screen.getByText("Said while recording.")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Transcript" })).toBeNull();

    // Stop pressed: still the live pane while Rust flushes the last lines.
    act(() => recording({ phase: "stopping", meetingId: ID, startedAtMs: 0 }));
    expect(screen.getByRole("heading", { name: "Live transcript" })).toBeTruthy();
    expect(readMeeting).toHaveBeenCalledTimes(1);

    readMeeting.mockResolvedValueOnce(detail([transcriptLine({ text: "Said while recording." })]));
    act(() => recording({ phase: "idle" }));

    await waitFor(() => expect(readMeeting).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("heading", { name: "Transcript" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Live transcript" })).toBeNull();
    expect(screen.getByText("00:00:04")).toBeTruthy();
    expect(screen.getByText("Said while recording.")).toBeTruthy();
  });

  test("another meeting recording leaves this one on its file", async () => {
    recording({ phase: "recording", meetingId: "2026-09-30-1100-meeting", startedAtMs: 0 });
    readMeeting.mockResolvedValueOnce(
      detail([transcriptLine({ time: "00:00:01", speaker: "Others", text: "From the file." })]),
    );
    renderReview();

    expect(await screen.findByRole("heading", { name: "Transcript" })).toBeTruthy();
    expect(screen.getByText("From the file.")).toBeTruthy();
    expect(screen.queryByText("Said while recording.")).toBeNull();

    // That other meeting ending is not a reason to re-read this one.
    act(() => recording({ phase: "idle" }));
    await Promise.resolve();
    expect(readMeeting).toHaveBeenCalledTimes(1);
  });
});
