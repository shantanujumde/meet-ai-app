import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Link, MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { AGENT_RUN_STATUS_EVENT } from "@/ipc/client";
import type { MeetingDetail, RecordingStatus } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { EMPTY_LIVE, useTranscriptStore } from "@/state/transcript";
import { meetingDetail, meetingSummary, transcriptLine } from "@/test/fixtures";
import { emit, ipc } from "@/test/ipcMock";
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
    status: {
      phase: "idle",
      meetingId: null,
      startedAtMs: null,
      pause: { pausedAtMs: null, pausedTotalMs: 0 },
      error: null,
      ...status,
    },
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

  test("the finished transcript sits in a focusable scroll box under its heading", async () => {
    recording({ phase: "idle" });
    readMeeting.mockResolvedValueOnce(detail([transcriptLine({ text: "From the file." })]));
    renderReview();

    const box = (await screen.findAllByRole("region", { name: "Transcript" })).find(
      (el) => el.getAttribute("tabindex") === "0",
    ) as HTMLElement;
    expect(box).toBeTruthy();
    expect(box.className).toContain("overflow-y-auto");
    expect(box.className).toContain("relative");
    expect(box.querySelector("ol.transcript")).not.toBeNull();
    expect(box.contains(screen.getByRole("heading", { name: "Transcript" }))).toBe(false);
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
        "No agent is set up. Paste this into Claude Code or Codex and it will write the notes.",
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

  test("is offered when the picked agent's CLI is missing", async () => {
    copyPromptFallback.mockResolvedValue(false);
    ipc.agentChoice.mockResolvedValueOnce({ harness: "codex", model: "", binaryPath: null });
    renderReview();

    expect(await screen.findByRole("button", { name: "Copy prompt" })).toBeTruthy();
  });

  test("is not offered when the picked agent's CLI is ready", async () => {
    copyPromptFallback.mockResolvedValue(false);
    renderReview();

    expect(await screen.findByRole("button", { name: "Show in Finder" })).toBeTruthy();
    await waitFor(() => expect(ipc.detectAgents).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Copy prompt" })).toBeNull();
  });

  test("is not offered if detecting the CLI fails", async () => {
    copyPromptFallback.mockResolvedValue(false);
    ipc.detectAgents.mockRejectedValueOnce({ domain: "app", kind: "io", message: "No." });
    renderReview();

    expect(await screen.findByRole("button", { name: "Show in Finder" })).toBeTruthy();
    await waitFor(() => expect(ipc.detectAgents).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "Copy prompt" })).toBeNull();
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

  /** TUR-103: the title renames in place. */
  async function openRename() {
    readMeeting.mockResolvedValue(
      meetingDetail({ summary: { id: ID, title: "Standup" }, lines: [transcriptLine()] }),
    );
    renderReview();
    fireEvent.click(await screen.findByRole("button", { name: /^Rename meeting/ }));
    return screen.getByRole<HTMLInputElement>("textbox", { name: "Meeting title" });
  }

  test("Enter saves a new title, which the heading then shows", async () => {
    const field = await openRename();
    expect(field.value).toBe("Standup");
    expect(document.activeElement).toBe(field);

    fireEvent.change(field, { target: { value: "  Budget review " } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(ipc.renameMeeting).toHaveBeenCalledWith(ID, "Budget review"));
    const header = screen.getByRole("banner");
    expect(
      await within(header).findByRole("heading", { level: 1, name: "Budget review" }),
    ).toBeTruthy();
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("button", { name: /^Rename meeting/ })),
    );
  });

  test("clicking away saves too", async () => {
    const field = await openRename();
    fireEvent.change(field, { target: { value: "Budget review" } });
    fireEvent.blur(field);
    await waitFor(() => expect(ipc.renameMeeting).toHaveBeenCalledWith(ID, "Budget review"));
    expect(ipc.renameMeeting).toHaveBeenCalledTimes(1);
  });

  test("Escape, a blank title or the same title saves nothing", async () => {
    const field = await openRename();
    fireEvent.change(field, { target: { value: "Budget review" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.getByRole("heading", { level: 1, name: "Standup" })).toBeTruthy();

    for (const value of ["   ", "Standup"]) {
      fireEvent.click(screen.getByRole("button", { name: /^Rename meeting/ }));
      const again = screen.getByRole("textbox", { name: "Meeting title" });
      fireEvent.change(again, { target: { value } });
      fireEvent.keyDown(again, { key: "Enter" });
      expect(screen.getByRole("heading", { level: 1, name: "Standup" })).toBeTruthy();
    }
    expect(ipc.renameMeeting).not.toHaveBeenCalled();
  });

  test("a rename that fails keeps the old title and says why", async () => {
    ipc.renameMeeting.mockRejectedValueOnce({
      domain: "app",
      kind: "io",
      message: "The disk is full.",
    });
    const field = await openRename();
    fireEvent.change(field, { target: { value: "Budget review" } });
    fireEvent.keyDown(field, { key: "Enter" });

    expect(await screen.findByText(/The disk is full/)).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1, name: "Standup" })).toBeTruthy();
  });

  test("a finished notes run shows the agent's title in the header", async () => {
    readMeeting.mockResolvedValueOnce(
      meetingDetail({ summary: { id: ID, title: "Meeting" }, lines: [transcriptLine()] }),
    );
    renderReview();
    expect(await screen.findByRole("heading", { level: 1, name: "Meeting" })).toBeTruthy();

    readMeeting.mockResolvedValueOnce(
      meetingDetail({
        summary: { id: ID, title: "Search release planning" },
        lines: [transcriptLine()],
      }),
    );
    act(() => emit(AGENT_RUN_STATUS_EVENT, { meetingId: ID, state: { state: "done", tasks: 0 } }));
    expect(
      await screen.findByRole("heading", { level: 1, name: "Search release planning" }),
    ).toBeTruthy();
  });

  /** TUR-107: the list hears of a calendar title from the folder watcher. */
  function listShows(title: string) {
    act(() =>
      useAppStore.setState({
        meetings: { root: "/m", rootExists: true, meetings: [meetingSummary({ id: ID, title })] },
      }),
    );
  }

  test("the calendar naming the open meeting shows in the header", async () => {
    readMeeting.mockResolvedValueOnce(
      meetingDetail({ summary: { id: ID, title: "Meeting" }, lines: [transcriptLine()] }),
    );
    renderReview();
    expect(await screen.findByRole("heading", { level: 1, name: "Meeting" })).toBeTruthy();

    readMeeting.mockResolvedValueOnce(
      meetingDetail({ summary: { id: ID, title: "Platform Standup" }, lines: [transcriptLine()] }),
    );
    listShows("Platform Standup");
    expect(await screen.findByRole("heading", { level: 1, name: "Platform Standup" })).toBeTruthy();

    // A list that agrees with the header reads nothing more.
    listShows("Platform Standup");
    expect(readMeeting).toHaveBeenCalledTimes(2);
  });

  test("a calendar title arriving during a rename does not replace the user's", async () => {
    let answer: (saved: string) => void = () => {};
    ipc.renameMeeting.mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          answer = resolve;
        }),
    );
    const field = await openRename();
    fireEvent.change(field, { target: { value: "Budget review" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(ipc.renameMeeting).toHaveBeenCalledWith(ID, "Budget review"));

    readMeeting.mockResolvedValue(
      meetingDetail({ summary: { id: ID, title: "Platform Standup" }, lines: [transcriptLine()] }),
    );
    listShows("Platform Standup");
    await act(async () => answer("Budget review"));

    expect(await screen.findByRole("heading", { level: 1, name: "Budget review" })).toBeTruthy();
    expect(readMeeting).toHaveBeenCalledTimes(1);
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

/**
 * TUR-150: switching meetings, and re-reading one, while reads are still out.
 * The screen shows only the newest answer for the meeting in the URL, and the
 * notes typed into it are never put back to what was on disk.
 */
describe("Review switching and re-reading", () => {
  const NEXT = "2026-09-30-1100-meeting";
  const NOTES = /^What you want to remember/;

  test("a slower read of the last meeting does not show under the next one's URL", async () => {
    let answerFirst: (detail: MeetingDetail) => void = () => {};
    readMeeting.mockImplementation((id) =>
      id === ID
        ? new Promise<MeetingDetail>((resolve) => {
            answerFirst = resolve;
          })
        : Promise.resolve(
            meetingDetail({
              summary: { id: NEXT, title: "Planning" },
              notes: "Planning notes.",
              lines: [transcriptLine({ text: "Said in planning." })],
            }),
          ),
    );
    render(
      <MemoryRouter initialEntries={[`/meetings/${ID}`]}>
        <Link to={`/meetings/${NEXT}`}>Open planning</Link>
        <Routes>
          <Route path="/meetings/:id" element={<Review />} />
        </Routes>
      </MemoryRouter>,
    );
    await waitFor(() => expect(readMeeting).toHaveBeenCalledWith(ID));

    fireEvent.click(screen.getByRole("link", { name: "Open planning" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Planning" })).toBeTruthy();

    await act(async () =>
      answerFirst(
        meetingDetail({
          summary: { id: ID, title: "Standup" },
          notes: "Standup notes.",
          lines: [transcriptLine({ text: "Said in standup." })],
        }),
      ),
    );
    expect(screen.getByRole("heading", { level: 1, name: "Planning" })).toBeTruthy();
    expect(screen.queryByRole("heading", { level: 1, name: "Standup" })).toBeNull();
    expect(screen.getByText("Said in planning.")).toBeTruthy();
    expect(screen.queryByText("Said in standup.")).toBeNull();
    expect(screen.getByPlaceholderText(NOTES)).toHaveValue("Planning notes.");

    // Renaming the title on screen renames that meeting, not another.
    fireEvent.click(screen.getByRole("button", { name: /^Rename meeting/ }));
    const field = screen.getByRole<HTMLInputElement>("textbox", { name: "Meeting title" });
    expect(field.value).toBe("Planning");
    fireEvent.change(field, { target: { value: "Budget review" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(ipc.renameMeeting).toHaveBeenCalledWith(NEXT, "Budget review"));
    expect(ipc.renameMeeting).toHaveBeenCalledTimes(1);
  });

  test("a read that started first and lands last does not replace a newer one", async () => {
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    let answerFirst: (detail: MeetingDetail) => void = () => {};
    readMeeting.mockImplementationOnce(
      () =>
        new Promise<MeetingDetail>((resolve) => {
          answerFirst = resolve;
        }),
    );
    readMeeting.mockResolvedValueOnce(detail([transcriptLine({ text: "The finished file." })]));
    renderReview();
    await waitFor(() => expect(readMeeting).toHaveBeenCalledTimes(1));

    // The recording ends before the first read answers: a second read starts.
    act(() => recording({ phase: "idle" }));
    expect(await screen.findByText("The finished file.")).toBeTruthy();

    await act(async () => answerFirst(detail([])));
    expect(screen.getByText("The finished file.")).toBeTruthy();
    expect(screen.queryByText("Nothing was transcribed")).toBeNull();
  });

  test("notes typed while the meeting stops are kept, and saved, when it is re-read", async () => {
    recording({ phase: "recording", meetingId: ID, startedAtMs: 0 });
    readMeeting.mockResolvedValueOnce(meetingDetail({ summary: { id: ID }, notes: "" }));
    renderReview();

    const notes = await screen.findByPlaceholderText(NOTES);
    fireEvent.change(notes, { target: { value: "abc" } });
    fireEvent.blur(notes);
    await waitFor(() => expect(ipc.saveNotes).toHaveBeenCalledWith(ID, "abc"));
    fireEvent.change(notes, { target: { value: "abcdef" } });

    // Stopped before "def" was saved: the re-read finds only "abc" on disk.
    readMeeting.mockResolvedValueOnce(
      meetingDetail({
        summary: { id: ID },
        notes: "abc",
        lines: [transcriptLine({ text: "From the re-read." })],
      }),
    );
    act(() => recording({ phase: "stopping", meetingId: ID, startedAtMs: 0 }));
    act(() => recording({ phase: "idle" }));
    expect(await screen.findByText("From the re-read.")).toBeTruthy();

    const after = screen.getByPlaceholderText(NOTES);
    expect(after).toHaveValue("abcdef");
    fireEvent.blur(after);
    await waitFor(() => expect(ipc.saveNotes).toHaveBeenLastCalledWith(ID, "abcdef"));
  });
});

describe("Review: opening another meeting (TUR-171)", () => {
  test("asks the agent setup and runs the CLI check once per session, not per meeting", async () => {
    copyPromptFallback.mockResolvedValue(false);
    readMeeting.mockImplementation(async (id) => meetingDetail({ summary: { id } }));
    const first = renderReview(ID);
    expect(await screen.findByRole("button", { name: "Show in Finder" })).toBeTruthy();
    await waitFor(() => expect(ipc.detectAgents).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(ipc.notesAutoRun).toHaveBeenCalledTimes(1));
    first.unmount();

    renderReview("2026-09-30-1130-meeting");
    expect(await screen.findByRole("button", { name: "Show in Finder" })).toBeTruthy();
    await act(async () => {});
    expect(copyPromptFallback).toHaveBeenCalledTimes(1);
    expect(ipc.agentChoice).toHaveBeenCalledTimes(1);
    expect(ipc.detectAgents).toHaveBeenCalledTimes(1);
    expect(ipc.notesAutoRun).toHaveBeenCalledTimes(1);
  });
});
