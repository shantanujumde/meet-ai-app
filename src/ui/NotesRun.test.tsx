import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { AGENT_RUN_STATUS_EVENT } from "@/ipc/client";
import type { MeetingNotes, NotesRunState, NotesRunStatus } from "@/ipc/types";
import { emit, ipc } from "@/test/ipcMock";
import { NotesRun } from "./NotesRun";

/**
 * TUR-10: a meeting's notes run as the meeting view shows it — Writing notes…
 * with Cancel, then the notes or the reason there are none with Retry.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { notesRunStatus, startNotesRun, cancelNotesRun, meetingNotes } = ipc;

const ID = "2026-09-30-1015-meeting";

const NOTES: MeetingNotes = {
  notesOff: false,
  analyzedBy: "claude-code",
  sections: [
    { heading: "Summary", body: "We agreed to ship on Friday." },
    { heading: "Action Items", body: "- Shantanu: write the release notes" },
  ],
};

function status(state: NotesRunState, meetingId = ID): NotesRunStatus {
  return { meetingId, state };
}

function given(state: NotesRunState, notes?: MeetingNotes) {
  notesRunStatus.mockResolvedValue(status(state));
  if (notes) meetingNotes.mockResolvedValue(notes);
}

function renderRun({ canStart = true, onDone }: { canStart?: boolean; onDone?: () => void } = {}) {
  return render(<NotesRun meetingId={ID} canStart={canStart} onDone={onDone} />);
}

/** Wait until both first answers are in, so a test is past the blank render. */
async function loaded() {
  await waitFor(() => expect(notesRunStatus).toHaveBeenCalledWith(ID));
  await waitFor(() => expect(meetingNotes).toHaveBeenCalledWith(ID));
  await act(async () => {});
}

describe("NotesRun", () => {
  test("running: says Writing notes… and Cancel cancels this meeting's run", async () => {
    given({ state: "running" });
    renderRun();

    expect(await screen.findByText("Writing notes…")).toBeTruthy();
    expect(screen.getByRole("status")).toHaveTextContent("Writing notes…");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(cancelNotesRun).toHaveBeenCalledWith(ID));

    // The default answer is the cancelled failure, with Retry to start again.
    expect(await screen.findByText("You cancelled the notes")).toBeTruthy();
    expect(screen.queryByText("Writing notes…")).toBeNull();
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  test("failed: shows Rust's words and the command to type, and Retry starts it again", async () => {
    given({
      state: "failed",
      failure: {
        kind: "not-signed-in",
        message: "Claude Code is installed but nobody is signed in to it.",
        command: "claude",
      },
    });
    renderRun();

    expect(await screen.findByText("Your agent is not signed in")).toBeTruthy();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Claude Code is installed but nobody is signed in to it.",
    );
    expect(screen.getByText("claude").tagName).toBe("CODE");

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(startNotesRun).toHaveBeenCalledWith(ID));
    expect(await screen.findByText("Writing notes…")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  test("Retry is disabled while its start is on its way, so a second press does nothing", async () => {
    given({
      state: "failed",
      failure: { kind: "timed-out", message: "It ran past 10 minutes.", command: null },
    });
    let answer: (value: NotesRunStatus) => void = () => {};
    startNotesRun.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    renderRun();

    const retry = await screen.findByRole("button", { name: "Retry" });
    fireEvent.click(retry);
    await waitFor(() => expect(retry).toBeDisabled());
    fireEvent.click(retry);
    expect(startNotesRun).toHaveBeenCalledTimes(1);

    await act(async () => answer(status({ state: "running" })));
    expect(await screen.findByText("Writing notes…")).toBeTruthy();
  });

  test("done: the event refetches the notes and shows them", async () => {
    given({ state: "running" });
    const onDone = vi.fn();
    renderRun({ onDone });
    expect(await screen.findByText("Writing notes…")).toBeTruthy();
    expect(meetingNotes).toHaveBeenCalledTimes(1);

    meetingNotes.mockResolvedValue(NOTES);
    act(() => emit(AGENT_RUN_STATUS_EVENT, status({ state: "done", tasks: 2 })));

    expect(await screen.findByRole("heading", { name: "Summary" })).toBeTruthy();
    expect(meetingNotes).toHaveBeenCalledTimes(2);
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(screen.getByText("We agreed to ship on Friday.")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Action Items" })).toBeTruthy();
    expect(screen.getByText("Written by Claude Code into meeting.md")).toBeTruthy();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Notes written, with 2 tasks added to Tickets.",
    );
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  test("idle with no notes (a relaunch mid-run): offers to write them", async () => {
    renderRun();

    expect(await screen.findByText("No notes yet")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Write notes" }));
    await waitFor(() => expect(startNotesRun).toHaveBeenCalledWith(ID));
    expect(await screen.findByText("Writing notes…")).toBeTruthy();
  });

  test("idle with notes on disk: shows them and no start button", async () => {
    given({ state: "idle" }, NOTES);
    renderRun();

    expect(await screen.findByRole("heading", { name: "Summary" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("an event for another meeting is ignored", async () => {
    given({ state: "running" });
    renderRun();
    expect(await screen.findByText("Writing notes…")).toBeTruthy();

    meetingNotes.mockResolvedValue(NOTES);
    act(() =>
      emit(AGENT_RUN_STATUS_EVENT, status({ state: "done", tasks: 1 }, "2026-09-30-1100-other")),
    );
    await act(async () => {});

    expect(screen.getByText("Writing notes…")).toBeTruthy();
    expect(meetingNotes).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("heading", { name: "Summary" })).toBeNull();
  });

  test("an event that beats the first status answer is not overwritten by it", async () => {
    let answer: (value: NotesRunStatus) => void = () => {};
    notesRunStatus.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    renderRun();
    await waitFor(() => expect(notesRunStatus).toHaveBeenCalled());

    act(() => emit(AGENT_RUN_STATUS_EVENT, status({ state: "running" })));
    await act(async () => answer(status({ state: "idle" })));

    expect(await screen.findByText("Writing notes…")).toBeTruthy();
    expect(screen.queryByText("No notes yet")).toBeNull();
  });

  test("notes switched off: nothing about runs, and no start button", async () => {
    given({ state: "idle" }, { notesOff: true, analyzedBy: null, sections: [] });
    const { container } = renderRun();
    await loaded();

    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
    expect(screen.queryByText("No notes yet")).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });

  test("with no agent to run (Copy prompt instead): no start button and no Retry", async () => {
    given({
      state: "failed",
      failure: {
        kind: "no-agent",
        message: "No agent is set up. Copy the prompt and paste it into one.",
        command: null,
      },
    });
    renderRun({ canStart: false });

    expect(await screen.findByText("No agent is set up")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  test("idle with no notes and nothing to start from shows nothing", async () => {
    const { container } = renderRun({ canStart: false });
    await loaded();

    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });

  test("a start Rust refuses is shown as an error", async () => {
    startNotesRun.mockRejectedValue({ domain: "app", kind: "config", message: "Bad TOML." });
    renderRun();

    fireEvent.click(await screen.findByRole("button", { name: "Write notes" }));
    expect(await screen.findByText("Bad TOML.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Write notes" })).not.toBeDisabled();
  });
});
