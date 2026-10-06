import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { DETECTION_PROMPT_EVENT, type DetectionPrompt as Prompt } from "@/ipc/client";
import type { RecordingStatus } from "@/ipc/types";
import { ONBOARDING } from "@/lib/routes";
import { useRecordingStore } from "@/state/recording";
import { emit, ipc } from "@/test/ipcMock";
import { DetectionPrompt } from "./DetectionPrompt";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const ZOOM: Prompt = {
  signal: { kind: "process", process: "zoom.us" },
  reason: "Zoom is open.",
  updateOnly: false,
  eventId: null,
  canJoin: false,
  test: false,
  title: null,
  startsAtMs: null,
  endsAtMs: null,
  joinService: null,
};

const STANDUP: Prompt = {
  signal: { kind: "calendar", title: "Team standup", attendees: 3 },
  reason: "“Team standup” starts in 1 min, with 3 people invited.",
  updateOnly: false,
  eventId: "standup-1",
  canJoin: false,
  test: false,
  title: "Team standup",
  startsAtMs: null,
  endsAtMs: null,
  joinService: null,
};

/** A reminder for an event with a meeting link (TUR-78). */
const WITH_LINK: Prompt = { ...STANDUP, canJoin: true };

/** Settings' "Send a test reminder". */
const TEST: Prompt = {
  signal: { kind: "calendar", title: "Test meeting", attendees: 0 },
  reason: "“Test meeting” starts in 2 min. This is a test reminder: nothing will be recorded.",
  updateOnly: false,
  eventId: null,
  canJoin: true,
  test: true,
  title: "Test meeting",
  startsAtMs: null,
  endsAtMs: null,
  joinService: null,
};

/** The banner's buttons, in order. */
function buttonNames(): string[] {
  return screen.getAllByRole("button").map((button) => button.textContent ?? "");
}

/** Where the window is, so a test can see a navigation. */
function Where() {
  const location = useLocation();
  return <output aria-label="location">{location.pathname + location.search}</output>;
}

const RECORDING: RecordingStatus = {
  phase: "recording",
  meetingId: "2026-10-03-1800-meeting",
  startedAtMs: 1,
  error: null,
};

function show(path = "/meetings") {
  render(
    <MemoryRouter initialEntries={[path]}>
      <DetectionPrompt />
      <Routes>
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
}

function prompt(payload: Prompt = ZOOM) {
  act(() => emit(DETECTION_PROMPT_EVENT, payload));
}

describe("DetectionPrompt", () => {
  test("shows nothing until Rust asks", () => {
    show();
    expect(screen.queryByRole("region")).toBeNull();
    expect(screen.queryByRole("button", { name: "Record" })).toBeNull();
  });

  test("says why it is asking, and Record starts a recording", async () => {
    show();
    prompt();

    expect(screen.getByRole("region", { name: "Record this meeting?" })).toBeTruthy();
    expect(screen.getByText("Zoom is open.")).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Record" }));
    });
    expect(ipc.toggleRecording).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("Dismiss closes it without recording", () => {
    show();
    prompt();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("region")).toBeNull();
    expect(ipc.toggleRecording).not.toHaveBeenCalled();
  });

  test("no prompt while recording", () => {
    useRecordingStore.setState({ status: RECORDING });
    show();
    prompt();
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("a recording started elsewhere answers the prompt", () => {
    show();
    prompt();
    act(() => useRecordingStore.setState({ status: RECORDING }));
    expect(screen.queryByRole("region")).toBeNull();
    // And it does not come back when that recording ends.
    act(() => useRecordingStore.setState({ status: { ...RECORDING, phase: "idle" } }));
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("waits while onboarding owns the window", () => {
    show(ONBOARDING);
    prompt();
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("a meeting app's prompt has no brief to open", () => {
    show();
    prompt();
    expect(screen.queryByRole("button", { name: "Open brief" })).toBeNull();
  });

  test("a calendar reminder opens that meeting's brief and stays up", () => {
    show();
    prompt(STANDUP);
    expect(screen.getByText(STANDUP.reason)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open brief" }));
    expect(screen.getByRole("status", { name: "location" }).textContent).toBe(
      "/brief?title=Team+standup",
    );
    // Still there to answer, after reading the brief.
    expect(screen.getByRole("button", { name: "Record" })).toBeTruthy();
    expect(ipc.toggleRecording).not.toHaveBeenCalled();
  });

  test("Record from a reminder records that meeting, without joining", async () => {
    show();
    prompt(STANDUP);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Record" }));
    });
    expect(ipc.recordRemindedMeeting).toHaveBeenCalledWith("standup-1", false);
    expect(ipc.toggleRecording).not.toHaveBeenCalled();
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("a reminder without a link has no Join", () => {
    show();
    prompt(STANDUP);
    expect(buttonNames()).toEqual(["Record", "Open brief", "Dismiss"]);
  });

  test("a reminder with a link leads with Join and record, in order", () => {
    show();
    prompt(WITH_LINK);
    expect(buttonNames()).toEqual(["Join and record", "Join", "Record", "Open brief", "Dismiss"]);
    expect(screen.getByRole("button", { name: "Join and record" }).className).toContain(
      "bg-accent",
    );
    expect(screen.getByRole("button", { name: "Record" }).className).not.toContain("bg-accent");
    // Nothing runs without a click (L15).
    expect(ipc.recordRemindedMeeting).not.toHaveBeenCalled();
    expect(ipc.joinRemindedMeeting).not.toHaveBeenCalled();
  });

  test("Join and record joins and records that meeting", async () => {
    show();
    prompt(WITH_LINK);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Join and record" }));
    });
    expect(ipc.recordRemindedMeeting).toHaveBeenCalledWith("standup-1", true);
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("Join opens the meeting and leaves the question up", async () => {
    show();
    prompt(WITH_LINK);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Join" }));
    });
    expect(ipc.joinRemindedMeeting).toHaveBeenCalledWith("standup-1");
    expect(ipc.recordRemindedMeeting).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Record" })).toBeTruthy();
  });

  test("a refused start shows where the record button's would", async () => {
    ipc.recordRemindedMeeting.mockRejectedValueOnce({
      domain: "app",
      kind: "folder-moving",
      message: "The meetings folder is moving.",
    });
    show();
    prompt(WITH_LINK);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Record" }));
    });
    expect(useRecordingStore.getState().error?.message).toBe("The meetings folder is moving.");
    expect(useRecordingStore.getState().busy).toBe(false);
  });

  test("a test reminder looks like one and never records or joins", async () => {
    for (const name of ["Join and record", "Join", "Record", "Open brief"]) {
      show();
      prompt(TEST);
      expect(screen.getByText(TEST.reason)).toBeTruthy();
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name }));
      });
      expect(screen.queryByRole("region"), name).toBeNull();
      cleanup();
    }
    expect(ipc.recordRemindedMeeting).not.toHaveBeenCalled();
    expect(ipc.joinRemindedMeeting).not.toHaveBeenCalled();
    expect(ipc.toggleRecording).not.toHaveBeenCalled();
  });

  test("the same call noticed again replaces the banner on screen", () => {
    show();
    prompt();
    prompt({ ...STANDUP, updateOnly: true });
    expect(screen.queryByText("Zoom is open.")).toBeNull();
    expect(screen.getByText(STANDUP.reason)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Open brief" })).toBeTruthy();
  });

  test("the same call noticed again never brings back a dismissed banner", () => {
    show();
    prompt();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    prompt({ ...STANDUP, updateOnly: true });
    expect(screen.queryByRole("region")).toBeNull();
  });
});
