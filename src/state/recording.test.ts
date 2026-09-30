import { beforeEach, expect, it, vi } from "vitest";
import type { RecordingStatus, UiError } from "@/ipc/types";

const handlers: {
  state?: (status: RecordingStatus) => void;
  error?: (error: UiError) => void;
} = {};
const teardown = { state: vi.fn(), error: vi.fn() };

vi.mock("@/ipc/client", () => ({
  recordingStatus: vi.fn().mockResolvedValue({ phase: "idle", meetingId: null, startedAtMs: null }),
  toggleRecording: vi.fn(),
  onRecordingState: (handler: (status: RecordingStatus) => void) => {
    handlers.state = handler;
    return teardown.state;
  },
  onRecordingError: (handler: (error: UiError) => void) => {
    handlers.error = handler;
    return teardown.error;
  },
}));

const { useRecordingStore, watchRecordingState } = await import("./recording");

beforeEach(() => {
  useRecordingStore.setState({ error: null });
  teardown.state.mockClear();
  teardown.error.mockClear();
});

it("shows a recording Rust stopped by itself in the same error banner (TUR-97)", () => {
  const stop = watchRecordingState();
  const interrupted: UiError = {
    domain: "app",
    kind: "recording-interrupted",
    message: "The recording stopped because of an error (mic fsync: disk full).",
  };

  handlers.error?.(interrupted);
  expect(useRecordingStore.getState().error).toEqual(interrupted);

  stop();
  expect(teardown.state).toHaveBeenCalledOnce();
  expect(teardown.error).toHaveBeenCalledOnce();
});
