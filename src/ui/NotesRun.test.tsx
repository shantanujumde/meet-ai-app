import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { useNotesRun } from "@/hooks/useNotesRun";
import { AGENT_RUN_STATUS_EVENT } from "@/ipc/client";
import type { MeetingNotes, NotesRunState, NotesRunStatus } from "@/ipc/types";
import { emit, ipc } from "@/test/ipcMock";
import { NotesRun } from "./NotesRun";
import { NotesSwitch } from "./NotesSwitch";

/**
 * TUR-10: a meeting's notes run as the meeting view shows it — Writing notes…
 * with Cancel, then the notes or the reason there are none with Retry.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { notesRunStatus, startNotesRun, cancelNotesRun, meetingNotes, setMeetingNotes } = ipc;

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

/**
 * The run pane with the page's hook behind it, as the meeting view has it.
 * `withSwitch` adds the TUR-12 switch over it, wired the same way.
 */
function Page({
  canStart,
  onDone,
  withSwitch,
}: {
  canStart: boolean;
  onDone?: () => void;
  withSwitch: boolean;
}) {
  const run = useNotesRun(ID, onDone);
  const pane = <NotesRun run={run} canStart={canStart} />;
  if (!withSwitch) return pane;
  return (
    <>
      <NotesSwitch
        on={run.notes ? !run.notes.notesOff : true}
        busy={run.switching}
        error={run.switchError}
        onChange={run.setNotesOn}
      />
      {pane}
    </>
  );
}

function renderRun({
  canStart = true,
  onDone,
  withSwitch = false,
}: {
  canStart?: boolean;
  onDone?: () => void;
  withSwitch?: boolean;
} = {}) {
  return render(<Page canStart={canStart} onDone={onDone} withSwitch={withSwitch} />);
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

/**
 * TUR-12: the "Make notes for this meeting" switch, and what the pane shows
 * as it moves.
 */
describe("the notes switch", () => {
  const OFF: MeetingNotes = { notesOff: true, analyzedBy: null, sections: [] };

  function theSwitch() {
    return screen.getByRole("switch", { name: "Make notes for this meeting" });
  }

  test("is on by default; off saves it, says so, and hides the run and its start button", async () => {
    const onDone = vi.fn();
    renderRun({ withSwitch: true, onDone });
    expect(await screen.findByRole("button", { name: "Write notes" })).toBeTruthy();
    expect(theSwitch()).toHaveAttribute("aria-checked", "true");

    fireEvent.click(theSwitch());
    await waitFor(() => expect(setMeetingNotes).toHaveBeenCalledWith(ID, false));

    await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "false"));
    expect(theSwitch()).toHaveAccessibleDescription(
      "Off: the transcript is not sent to your agent. For private calls.",
    );
    expect(screen.queryByRole("button", { name: "Write notes" })).toBeNull();
    expect(screen.queryByText("No notes yet")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Meeting notes" })).toBeNull();
    // The list's "Notes off" marker reads from the list, so it is reloaded.
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  test("waits for its answer, so a second press while saving does nothing", async () => {
    let answer: (value: MeetingNotes) => void = () => {};
    setMeetingNotes.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    renderRun({ withSwitch: true });
    await loaded();

    fireEvent.click(theSwitch());
    await waitFor(() => expect(theSwitch()).toBeDisabled());
    fireEvent.click(theSwitch());
    expect(setMeetingNotes).toHaveBeenCalledTimes(1);

    await act(async () => answer(OFF));
    expect(theSwitch()).not.toBeDisabled();
    expect(theSwitch()).toHaveAttribute("aria-checked", "false");
  });

  test("off, then on again: offers Make notes now, which starts the run", async () => {
    given({ state: "idle" }, OFF);
    renderRun({ withSwitch: true });
    await loaded();
    expect(theSwitch()).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("button", { name: "Make notes now" })).toBeNull();

    fireEvent.click(theSwitch());
    await waitFor(() => expect(setMeetingNotes).toHaveBeenCalledWith(ID, true));

    expect(await screen.findByText("Notes are on for this meeting")).toBeTruthy();
    expect(theSwitch()).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("button", { name: "Make notes now" }));
    await waitFor(() => expect(startNotesRun).toHaveBeenCalledWith(ID));
    expect(await screen.findByText("Writing notes…")).toBeTruthy();
  });

  test("a run refused because notes were off reads as Make notes now once they are on", async () => {
    given(
      {
        state: "failed",
        failure: { kind: "notes-off", message: "Notes are off for this meeting.", command: null },
      },
      OFF,
    );
    renderRun({ withSwitch: true });
    await loaded();

    fireEvent.click(theSwitch());
    expect(await screen.findByRole("button", { name: "Make notes now" })).toBeTruthy();
    expect(screen.queryByText("Notes are off for this meeting")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  test("switched off mid-run, the cancelled run says nothing; back on, Make notes now", async () => {
    given({ state: "running" });
    renderRun({ withSwitch: true });
    expect(await screen.findByText("Writing notes…")).toBeTruthy();

    fireEvent.click(theSwitch());
    await waitFor(() => expect(setMeetingNotes).toHaveBeenCalledWith(ID, false));
    // Rust cancels the run as it writes the switch, and says so.
    act(() =>
      emit(
        AGENT_RUN_STATUS_EVENT,
        status({
          state: "failed",
          failure: { kind: "cancelled", message: "Cancelled.", command: null },
        }),
      ),
    );
    await waitFor(() => expect(screen.queryByText("Writing notes…")).toBeNull());
    expect(screen.queryByText("You cancelled the notes")).toBeNull();

    fireEvent.click(theSwitch());
    expect(await screen.findByRole("button", { name: "Make notes now" })).toBeTruthy();
    expect(screen.queryByText("You cancelled the notes")).toBeNull();
  });

  test("a switch Rust cannot save shows why and stays where it was", async () => {
    setMeetingNotes.mockRejectedValue({ domain: "app", kind: "config", message: "Bad TOML." });
    renderRun({ withSwitch: true });
    await loaded();

    fireEvent.click(theSwitch());
    expect(await screen.findByText("Bad TOML.")).toBeTruthy();
    expect(theSwitch()).toHaveAttribute("aria-checked", "true");
    expect(theSwitch()).not.toBeDisabled();
  });
});
