import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { MeetingList, RecordingStatus } from "@/ipc/types";
import { Review } from "@/routes/Review";
import { useAppStore } from "@/state/app";
import { meetingDetail, meetingSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { Sidebar } from "./Sidebar";

/**
 * TUR-166: typing notes autosaves every 600 ms pause, and each save refreshes
 * the meeting list. That refresh is a background one: the sidebar keeps its
 * rows on screen the whole time, never swapping them for the "Reading your
 * meetings folder…" spinner, which also threw away the list's scroll position.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const ID = "2026-09-30-1015-meeting";
const IDLE: RecordingStatus = { phase: "idle", meetingId: null, startedAtMs: null, error: null };
const LIST: MeetingList = {
  root: "/Users/test/Meetings",
  rootExists: true,
  meetings: [
    meetingSummary({ id: ID, title: "Platform standup" }),
    meetingSummary({ id: "2026-09-29-0900-review", title: "Design review" }),
  ],
};
const SPINNER = "Reading your meetings folder…";

/** The sidebar fed from the app store, beside the open meeting, as in the shell. */
function Harness() {
  const meetings = useAppStore((state) => state.meetings);
  const loading = useAppStore((state) => state.meetingsLoading);
  return (
    <>
      <Sidebar list={meetings} loading={loading} recording={IDLE} selectedId={ID} />
      <Routes>
        <Route path="/meetings/:id" element={<Review />} />
      </Routes>
    </>
  );
}

describe("Sidebar while notes are typed (TUR-166)", () => {
  beforeEach(() => {
    useAppStore.setState({ meetings: LIST, meetingsLoading: false, meetingsError: null });
    ipc.listMeetings.mockResolvedValue(LIST);
    ipc.readMeeting.mockResolvedValue(meetingDetail({ summary: { id: ID } }));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  test("an autosave refreshes the list without the spinner or the rows going away", async () => {
    render(
      <MemoryRouter initialEntries={[`/meetings/${ID}`]}>
        <Harness />
      </MemoryRouter>,
    );
    const nav = screen.getByRole("navigation", { name: "Sidebar" });
    const notes = await screen.findByPlaceholderText<HTMLTextAreaElement>(
      /What you want to remember/,
    );

    // Any moment the store calls the list loading would put the spinner up.
    let flashed = false;
    const stop = useAppStore.subscribe((state) => {
      if (state.meetingsLoading) flashed = true;
    });
    ipc.listMeetings.mockClear();

    vi.useFakeTimers();
    for (const typed of ["a", "ab", "abc"]) {
      fireEvent.change(notes, { target: { value: typed } });
      expect(within(nav).queryByText(SPINNER)).toBeNull();
      await act(async () => {
        vi.advanceTimersByTime(600);
      });
      expect(within(nav).queryByText(SPINNER)).toBeNull();
      expect(within(nav).getByText("Design review")).toBeInTheDocument();
    }
    vi.useRealTimers();
    stop();

    expect(ipc.saveNotes).toHaveBeenLastCalledWith(ID, "abc");
    await waitFor(() => expect(ipc.listMeetings).toHaveBeenCalled());
    expect(flashed).toBe(false);
    expect(within(nav).getByText("Platform standup")).toBeInTheDocument();
  });

  test("a loading flag with a list already shown keeps the rows", () => {
    render(
      <MemoryRouter initialEntries={["/meetings"]}>
        <Sidebar list={LIST} loading recording={IDLE} selectedId={undefined} />
      </MemoryRouter>,
    );
    const nav = screen.getByRole("navigation", { name: "Sidebar" });
    expect(within(nav).queryByText(SPINNER)).toBeNull();
    expect(within(nav).getByText("Design review")).toBeInTheDocument();
  });

  test("the first load, with no list yet, shows the spinner", () => {
    render(
      <MemoryRouter initialEntries={["/meetings"]}>
        <Sidebar list={null} loading recording={IDLE} selectedId={undefined} />
      </MemoryRouter>,
    );
    expect(screen.getByText(SPINNER)).toBeInTheDocument();
  });
});
