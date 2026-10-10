import { act, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { describe, expect, test, vi } from "vitest";
import type { RecordingStatus } from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";
import { EMPTY_LIVE, useTranscriptStore } from "@/state/transcript";
import { meetingDetail, transcriptLine } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { Review } from "./Review";

/**
 * TUR-171: the live transcript changes several times a second while a
 * meeting records. Only the live pane may re-render for it; the header (and
 * with it the notes, the tasks and the rest of the page) must not.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const renders = vi.hoisted(() => ({ header: 0 }));
vi.mock("@/ui/MeetingHeader", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/ui/MeetingHeader")>();
  return {
    ...actual,
    MeetingHeader: (props: Parameters<typeof actual.MeetingHeader>[0]) => {
      renders.header += 1;
      return actual.MeetingHeader(props);
    },
  };
});

const ID = "2026-09-30-1015-meeting";

function recording(status: Partial<Omit<RecordingStatus, "error">>) {
  useRecordingStore.setState({
    status: { phase: "idle", meetingId: null, startedAtMs: null, error: null, ...status },
  });
}

/** One more live line, as a transcript event would add. */
function liveLine(seq: number) {
  useTranscriptStore.setState((state) => ({
    live: {
      ...state.live,
      status: { state: "running", engine: "apple-speech", detail: null },
      finals: [
        ...state.live.finals,
        { seq, speaker: "you", start_sec: seq, text: `Live line ${seq}.` },
      ],
    },
  }));
}

async function open() {
  render(
    <MemoryRouter initialEntries={[`/meetings/${ID}`]}>
      <Routes>
        <Route path="/meetings/:id" element={<Review />} />
      </Routes>
    </MemoryRouter>,
  );
  await screen.findByRole("heading", { level: 1 });
  await act(async () => {});
}

describe("Review re-renders (TUR-171)", () => {
  test("live-transcript events re-render the live pane, not the page", async () => {
    useTranscriptStore.setState({ live: EMPTY_LIVE });
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    ipc.readMeeting.mockResolvedValue(meetingDetail({ summary: { id: ID }, lines: [] }));
    await open();
    const before = renders.header;

    for (const seq of [1, 2, 3, 4, 5]) act(() => liveLine(seq));

    expect(await screen.findByText("Live line 5.")).toBeTruthy();
    expect(renders.header).toBe(before);
  });

  test("another meeting recording leaves a finished meeting's page alone", async () => {
    useTranscriptStore.setState({ live: EMPTY_LIVE });
    recording({ phase: "recording", meetingId: "someone-else", startedAtMs: 0 });
    ipc.readMeeting.mockResolvedValue(
      meetingDetail({ summary: { id: ID }, lines: [transcriptLine({ text: "Done." })] }),
    );
    await open();
    const before = renders.header;

    for (const seq of [1, 2, 3]) act(() => liveLine(seq));

    expect(renders.header).toBe(before);
    expect(screen.queryByText("Live line 3.")).toBeNull();
  });
});
