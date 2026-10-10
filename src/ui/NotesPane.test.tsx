import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { NotesPane } from "./NotesPane";

/**
 * TUR-150: the notes on disk are read into the pane once. A re-read of the
 * same meeting (its recording stopping, say) must never put older text back
 * over what the user typed since, and a save still waiting must still happen.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { saveNotes } = ipc;

const A = "2026-09-30-1015-meeting";
const B = "2026-09-30-1100-meeting";

function field(): HTMLTextAreaElement {
  return screen.getByRole<HTMLTextAreaElement>("textbox");
}

/** Past the autosave debounce, with the save's promise settled. */
async function autosave() {
  await act(async () => {
    vi.advanceTimersByTime(600);
  });
}

describe("NotesPane", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  test("a re-read of the same meeting keeps what was typed after the last save", async () => {
    const { rerender } = render(<NotesPane key={A} meetingId={A} initialNotes="" />);

    fireEvent.change(field(), { target: { value: "abc" } });
    await autosave();
    expect(saveNotes).toHaveBeenCalledWith(A, "abc");

    // "def" is typed, then the parent re-reads the meeting before the
    // debounce runs out: disk still says "abc".
    fireEvent.change(field(), { target: { value: "abcdef" } });
    rerender(<NotesPane key={A} meetingId={A} initialNotes="abc" />);
    expect(field().value).toBe("abcdef");

    await autosave();
    expect(saveNotes).toHaveBeenLastCalledWith(A, "abcdef");
    expect(screen.getByRole("status")).toHaveTextContent("Saved");
  });

  test("going away with a save still waiting writes it", () => {
    const { unmount } = render(<NotesPane key={A} meetingId={A} initialNotes="" />);

    fireEvent.change(field(), { target: { value: "draft" } });
    expect(saveNotes).not.toHaveBeenCalled();
    unmount();
    expect(saveNotes).toHaveBeenCalledWith(A, "draft");
  });

  test("another meeting gets its own notes, and the last one's waiting text goes to the last one", () => {
    const { rerender } = render(<NotesPane key={A} meetingId={A} initialNotes="" />);
    fireEvent.change(field(), { target: { value: "for A" } });

    rerender(<NotesPane key={B} meetingId={B} initialNotes="B's notes" />);
    expect(field().value).toBe("B's notes");
    expect(saveNotes).toHaveBeenCalledTimes(1);
    expect(saveNotes).toHaveBeenCalledWith(A, "for A");
  });

  test("nothing typed means nothing saved", async () => {
    const { rerender, unmount } = render(<NotesPane key={A} meetingId={A} initialNotes="old" />);
    rerender(<NotesPane key={A} meetingId={A} initialNotes="newer on disk" />);
    expect(field().value).toBe("old");
    await autosave();
    unmount();
    expect(saveNotes).not.toHaveBeenCalled();
  });
});
