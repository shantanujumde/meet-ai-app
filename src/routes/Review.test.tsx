import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { MeetingDetail, RecordingStatus } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
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

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

const { readMeeting, copyPromptFallback, wrapUpPrompt } = ipc;

const ID = "2026-09-30-1015-meeting";

function detail(lines: MeetingDetail["lines"]): MeetingDetail {
  return meetingDetail({ summary: { id: ID }, lines });
}

function recording(status: Partial<Omit<RecordingStatus, "error">>) {
  useRecordingStore.setState({
    status: { phase: "idle", meetingId: null, startedAtMs: null, error: null, ...status },
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

/**
 * A11's fallback: with no agent set up, the meeting offers its wrap-up prompt
 * on the clipboard instead.
 */
describe("Review's Copy prompt", () => {
  test("is offered when no agent is set up, and copies this meeting's prompt", async () => {
    vi.mocked(copyText).mockReset();
    copyPromptFallback.mockResolvedValue(true);
    wrapUpPrompt.mockImplementation(async (id) => `Wrap up ${id}.`);
    renderReview();

    const button = await screen.findByRole("button", { name: "Copy prompt" });
    expect(
      screen.getByText(
        "No agent set up — paste this into Claude Code or Codex and it will write the notes.",
      ),
    ).toBeTruthy();

    fireEvent.click(button);
    await waitFor(() => expect(wrapUpPrompt).toHaveBeenCalledWith(ID));
    await waitFor(() => expect(copyText).toHaveBeenCalledWith(`Wrap up ${ID}.`));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
  });

  test("is not offered when an agent is set up", async () => {
    copyPromptFallback.mockResolvedValue(false);
    renderReview();

    expect(await screen.findByRole("button", { name: "Show in Finder" })).toBeTruthy();
    await waitFor(() => expect(copyPromptFallback).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Copy prompt" })).toBeNull();
  });

  test("is not offered if asking fails", async () => {
    copyPromptFallback.mockRejectedValue({ domain: "app", kind: "config", message: "Bad TOML." });
    renderReview();

    expect(await screen.findByRole("button", { name: "Show in Finder" })).toBeTruthy();
    await waitFor(() => expect(copyPromptFallback).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Copy prompt" })).toBeNull();
    expect(screen.queryByText("Bad TOML.")).toBeNull();
  });

  test("is not offered while this meeting is recording", async () => {
    copyPromptFallback.mockResolvedValue(true);
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    renderReview();

    expect(await screen.findByRole("heading", { name: "Live transcript" })).toBeTruthy();
    await waitFor(() => expect(copyPromptFallback).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Copy prompt" })).toBeNull();
  });
});
