import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

/**
 * TUR-10: the agent's notes sit under the header once the meeting is not
 * recording, and the start button follows the same rule as Copy prompt.
 */
describe("Review's meeting notes", () => {
  test("offers to write notes for a finished meeting with a transcript", async () => {
    readMeeting.mockResolvedValue(detail([transcriptLine()]));
    renderReview();

    expect(await screen.findByRole("heading", { name: "Meeting notes" })).toBeTruthy();
    expect(await screen.findByRole("button", { name: "Write notes" })).toBeTruthy();
  });

  test("holds only the switch while this meeting records, and shows the run once it stops", async () => {
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    readMeeting.mockResolvedValue(detail([transcriptLine()]));
    ipc.notesRunStatus.mockResolvedValue({ meetingId: ID, state: { state: "running" } });
    renderReview();

    expect(await screen.findByRole("heading", { name: "Live transcript" })).toBeTruthy();
    const notes = screen.getByRole("region", { name: "Meeting notes" });
    expect(within(notes).getByRole("switch", { name: "Make notes for this meeting" })).toBeTruthy();
    await waitFor(() => expect(ipc.notesRunStatus).toHaveBeenCalledWith(ID));
    expect(screen.queryByText("Writing notes…")).toBeNull();

    act(() => recording({ phase: "idle" }));
    expect(await screen.findByText("Writing notes…")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
  });

  test("does not offer to start notes when Copy prompt stands in for the agent", async () => {
    copyPromptFallback.mockResolvedValue(true);
    readMeeting.mockResolvedValue(detail([transcriptLine()]));
    renderReview();

    expect(await screen.findByRole("button", { name: "Copy prompt" })).toBeTruthy();
    await waitFor(() => expect(ipc.meetingNotes).toHaveBeenCalledWith(ID));
    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
  });

  test("does not offer to start notes for a meeting with nothing transcribed", async () => {
    readMeeting.mockResolvedValue(detail([]));
    renderReview();

    expect(await screen.findByText("Nothing was transcribed")).toBeTruthy();
    await waitFor(() => expect(ipc.meetingNotes).toHaveBeenCalledWith(ID));
    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
  });
});

/**
 * TUR-12, SPEC A11: the "Make notes for this meeting" switch heads the
 * meeting-notes section (TUR-81), so it is there while recording as well as
 * after — and not in the header.
 */
describe("Review's notes switch", () => {
  const SWITCH = { name: "Make notes for this meeting" } as const;

  test("is there while this meeting records, and switching off saves it and reloads the list", async () => {
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    readMeeting.mockResolvedValue(detail([]));
    renderReview();

    expect(await screen.findByRole("heading", { name: "Live transcript" })).toBeTruthy();
    const toggle = within(screen.getByRole("region", { name: "Meeting notes" })).getByRole(
      "switch",
      SWITCH,
    );
    expect(within(screen.getByRole("banner")).queryByRole("switch")).toBeNull();
    expect(toggle).toHaveAccessibleDescription(
      "On: your agent writes notes from the transcript when the call ends.",
    );
    expect(toggle).toHaveAttribute("aria-checked", "true");

    fireEvent.click(toggle);
    await waitFor(() => expect(ipc.setMeetingNotes).toHaveBeenCalledWith(ID, false));
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    // The list's "Notes off" marker comes from a fresh read of the list.
    await waitFor(() => expect(ipc.listMeetings).toHaveBeenCalled());
  });

  test("a meeting already switched off opens off, with no Copy prompt and no start button", async () => {
    copyPromptFallback.mockResolvedValue(true);
    readMeeting.mockResolvedValue(
      meetingDetail({ summary: { id: ID, notesOff: true }, lines: [transcriptLine()] }),
    );
    ipc.meetingNotes.mockResolvedValue({ notesOff: true, analyzedBy: null, sections: [] });
    renderReview();

    expect(await screen.findByRole("switch", SWITCH)).toHaveAttribute("aria-checked", "false");
    await waitFor(() => expect(copyPromptFallback).toHaveBeenCalled());
    await waitFor(() => expect(ipc.meetingNotes).toHaveBeenCalledWith(ID));
    expect(screen.queryByRole("button", { name: "Copy prompt" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
  });

  test("switched back on for a finished meeting, offers Make notes now", async () => {
    readMeeting.mockResolvedValue(
      meetingDetail({ summary: { id: ID, notesOff: true }, lines: [transcriptLine()] }),
    );
    ipc.meetingNotes.mockResolvedValue({ notesOff: true, analyzedBy: null, sections: [] });
    renderReview();

    const toggle = await screen.findByRole("switch", SWITCH);
    await waitFor(() => expect(ipc.meetingNotes).toHaveBeenCalledWith(ID));
    fireEvent.click(toggle);

    fireEvent.click(await screen.findByRole("button", { name: "Make notes now" }));
    await waitFor(() => expect(ipc.startNotesRun).toHaveBeenCalledWith(ID));
  });
});

/**
 * TUR-81: the header is the title, one meta line and two icon actions. The
 * folder path is the Show in Finder tooltip, never printed in the header.
 */
describe("Review's header", () => {
  const PATH = `/Users/test/Meetings/${ID}`;

  test("shows the title, one meta line with the length, and no raw path", async () => {
    readMeeting.mockResolvedValue(
      meetingDetail({
        summary: { id: ID, title: "Standup", time: "22:36", audioMs: 50 * 60_000 },
        lines: [transcriptLine()],
      }),
    );
    renderReview();

    const header = await screen.findByRole("banner");
    expect(within(header).getByRole("heading", { level: 1, name: "Standup" })).toBeTruthy();
    const meta = within(header).getByTestId("meeting-meta");
    expect(meta).toHaveTextContent(/·\s*22:36\s*·\s*50 min/);
    expect(within(header).queryByText(PATH)).toBeNull();
  });

  test("Show in Finder is an icon button with the path as its tooltip, and opens the folder", async () => {
    renderReview();

    const button = await screen.findByRole("button", { name: "Show in Finder" });
    expect(button).toHaveAttribute("title", PATH);
    fireEvent.click(button);
    await waitFor(() => expect(ipc.revealMeeting).toHaveBeenCalledWith(ID));
  });

  test("Copy folder path copies the path and says Copied", async () => {
    vi.mocked(copyText).mockReset();
    renderReview();

    fireEvent.click(await screen.findByRole("button", { name: "Copy folder path" }));
    await waitFor(() => expect(copyText).toHaveBeenCalledWith(PATH));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
    expect(screen.getByText("Folder path copied to the clipboard")).toBeTruthy();
  });

  test("a clipboard that refuses shows the path to copy by hand", async () => {
    vi.mocked(copyText).mockReset();
    vi.mocked(copyText).mockRejectedValueOnce(new Error("no focus"));
    renderReview();

    fireEvent.click(await screen.findByRole("button", { name: "Copy folder path" }));
    expect(await screen.findByText(PATH)).toBeTruthy();
  });

  test("says Recording on the meta line while this meeting records", async () => {
    recording({ phase: "recording", meetingId: ID, startedAtMs: Date.now() - 12 * 60_000 });
    readMeeting.mockResolvedValue(detail([]));
    renderReview();

    const meta = await screen.findByTestId("meeting-meta");
    expect(meta).toHaveTextContent(/^Recording/);
    expect(meta).toHaveTextContent("12 min");
  });
});
