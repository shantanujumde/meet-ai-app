import { act, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { HOOK_FAILED_EVENT } from "@/ipc/client";
import { emit } from "@/test/ipcMock";
import { HookFailedNote } from "./HookFailedNote";

/** TUR-63: a failed user hook shows as a small note on its own meeting only. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

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
});
