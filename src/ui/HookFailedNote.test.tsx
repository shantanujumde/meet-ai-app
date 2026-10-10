import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { HOOK_FAILED_EVENT } from "@/ipc/client";
import { useHookFailures, watchHookFailures } from "@/state/hookFailures";
import { emit } from "@/test/ipcMock";
import { HookFailedNote } from "./HookFailedNote";

/** TUR-63: a failed user hook shows as a small note on its own meeting only. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

let stop: () => void = () => {};

beforeEach(() => {
  useHookFailures.setState({ byMeeting: {} });
  stop = watchHookFailures();
});

afterEach(() => stop());

describe("HookFailedNote", () => {
  test("shows a failed hook for this meeting, and not for another", () => {
    render(<HookFailedNote meetingId="m1" />);
    expect(screen.queryByText(/Hook failed/)).toBeNull();
    act(() => {
      emit(HOOK_FAILED_EVENT, { meetingId: "m2", hook: "on_meeting_end", message: "x" });
    });
    expect(screen.queryByText(/Hook failed/)).toBeNull();
    act(() => {
      emit(HOOK_FAILED_EVENT, {
        meetingId: "m1",
        hook: "on_meeting_end",
        message: "exited with status 3",
      });
    });
    expect(screen.getByText(/Hook failed: on_meeting_end \(exited with status 3\)/)).toBeTruthy();
  });

  // TUR-170: a hook that failed while the meeting was not open, and one
  // seen before, still show when the meeting is opened (again).
  test("shows failures from before the meeting view opened, every time it opens", () => {
    act(() => {
      emit(HOOK_FAILED_EVENT, { meetingId: "m1", hook: "on_meeting_end", message: "exit 1" });
    });
    const first = render(<HookFailedNote meetingId="m1" />);
    expect(screen.getByText(/Hook failed: on_meeting_end \(exit 1\)/)).toBeTruthy();
    first.unmount();

    render(<HookFailedNote meetingId="m1" />);
    expect(screen.getByText(/Hook failed: on_meeting_end \(exit 1\)/)).toBeTruthy();
  });

  test("keeps only the latest failure of each hook", () => {
    render(<HookFailedNote meetingId="m1" />);
    act(() => {
      emit(HOOK_FAILED_EVENT, { meetingId: "m1", hook: "on_meeting_end", message: "first" });
      emit(HOOK_FAILED_EVENT, { meetingId: "m1", hook: "on_meeting_end", message: "second" });
    });
    expect(screen.queryByText(/\(first\)/)).toBeNull();
    expect(screen.getByText(/\(second\)/)).toBeTruthy();
  });
});
