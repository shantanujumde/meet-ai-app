import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { HEADPHONE_WARNING_EVENT } from "@/ipc/client";
import type { RecordingStatus } from "@/ipc/types";
import { emit, ipc } from "@/test/ipcMock";
import { HeadphoneBanner, NO_HEADPHONES_TEXT } from "./HeadphoneBanner";

/** TUR-65: a quiet, dismissible "No headphones" line while recording through speakers. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const RECORDING: RecordingStatus = {
  phase: "recording",
  meetingId: "m1",
  startedAtMs: 1,
  error: null,
};

const IDLE: RecordingStatus = { phase: "idle", meetingId: null, startedAtMs: null, error: null };

function warn(show: boolean, meetingId = "m1") {
  act(() => {
    emit(HEADPHONE_WARNING_EVENT, { meetingId, show });
  });
}

describe("HeadphoneBanner", () => {
  test("speakers show the banner; headphones and no reading do not", () => {
    render(<HeadphoneBanner status={RECORDING} />);
    expect(screen.queryByText(NO_HEADPHONES_TEXT)).toBeNull();
    warn(true);
    expect(screen.getByRole("status").textContent).toContain(NO_HEADPHONES_TEXT);
    warn(false);
    expect(screen.queryByText(NO_HEADPHONES_TEXT)).toBeNull();
  });

  test("the text has no em dash", () => {
    expect(NO_HEADPHONES_TEXT).not.toContain("\u2014");
  });

  test("only for the meeting being recorded, and never when idle", () => {
    const { rerender } = render(<HeadphoneBanner status={RECORDING} />);
    warn(true, "m0");
    expect(screen.queryByText(NO_HEADPHONES_TEXT)).toBeNull();
    warn(true);
    expect(screen.getByText(NO_HEADPHONES_TEXT)).toBeTruthy();
    rerender(<HeadphoneBanner status={IDLE} />);
    expect(screen.queryByText(NO_HEADPHONES_TEXT)).toBeNull();
  });

  test("dismiss hides it until the output goes to headphones and back", () => {
    render(<HeadphoneBanner status={RECORDING} />);
    warn(true);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText(NO_HEADPHONES_TEXT)).toBeNull();
    // The same reading again (Rust only sends changes, but be safe).
    warn(true);
    expect(screen.queryByText(NO_HEADPHONES_TEXT)).toBeNull();
    warn(false);
    warn(true);
    expect(screen.getByText(NO_HEADPHONES_TEXT)).toBeTruthy();
  });

  test("a window opened mid-recording asks for the current reading", async () => {
    ipc.headphoneWarning.mockResolvedValue({ meetingId: "m1", show: true });
    render(<HeadphoneBanner status={RECORDING} />);
    await waitFor(() => expect(screen.getByText(NO_HEADPHONES_TEXT)).toBeTruthy());
  });
});
