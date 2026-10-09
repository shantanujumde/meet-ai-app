import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RecordingStatus, UiError } from "@/ipc/types";

/**
 * The recording store mirrors Rust. The one thing it adds is keeping the
 * reason a recording ended on its own (TUR-97), or a ⌘⇧R press was refused
 * (TUR-127), which arrives on the state event rather than from a button press.
 */

const handlers: { state?: (status: RecordingStatus) => void } = {};
const teardown = vi.fn();
const recordingStatus = vi.fn<() => Promise<RecordingStatus>>();
const toggleRecording = vi.fn<() => Promise<RecordingStatus>>();
const pauseRecording = vi.fn<() => Promise<RecordingStatus>>();
const resumeRecording = vi.fn<() => Promise<RecordingStatus>>();
const stopRecording = vi.fn<() => Promise<RecordingStatus>>();

// `@/lib/elapsed`, not `@/ipc/recordingPause`: that one imports `./client`,
// this very mock, so awaiting it inside the factory never settles.
vi.mock("@/ipc/client", async () => ({
  isPaused: (await import("@/lib/elapsed")).isPaused,
  recordingStatus: () => recordingStatus(),
  toggleRecording: () => toggleRecording(),
  pauseRecording: () => pauseRecording(),
  resumeRecording: () => resumeRecording(),
  stopRecording: () => stopRecording(),
  onRecordingState: (handler: (status: RecordingStatus) => void) => {
    handlers.state = handler;
    return teardown;
  },
}));

const { useRecordingStore, watchRecordingState } = await import("./recording");

const IDLE: RecordingStatus = { phase: "idle", meetingId: null, startedAtMs: null, error: null };
const STARTING: RecordingStatus = { ...IDLE, phase: "starting" };
const RECORDING: RecordingStatus = {
  phase: "recording",
  meetingId: "2026-09-30-1000-meeting",
  startedAtMs: 0,
  error: null,
};
const INTERRUPTED: UiError = {
  domain: "app",
  kind: "recording-interrupted",
  message: "The recording stopped because of an error (mic fsync: disk full).",
};

function push(status: RecordingStatus) {
  handlers.state?.(status);
}

describe("the recording store", () => {
  beforeEach(() => {
    useRecordingStore.setState({ status: IDLE, error: null, busy: false });
    recordingStatus.mockResolvedValue(IDLE);
    toggleRecording.mockReset();
    pauseRecording.mockReset();
    resumeRecording.mockReset();
    stopRecording.mockReset();
    teardown.mockClear();
    sessionStorage.clear();
  });

  it("shows a recording Rust stopped by itself in the same error banner (TUR-97)", () => {
    const stop = watchRecordingState();
    push(RECORDING);
    push({ ...IDLE, error: INTERRUPTED });
    expect(useRecordingStore.getState().status.phase).toBe("idle");
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);

    stop();
    expect(teardown).toHaveBeenCalledOnce();
  });

  it("keeps the shown error through a status that carries none", () => {
    const stop = watchRecordingState();
    useRecordingStore.setState({ error: INTERRUPTED });
    push(IDLE);
    push({ ...IDLE, error: null });
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);
    stop();
  });

  it("clears the old reason when a new start begins, from any surface", () => {
    const stop = watchRecordingState();
    push({ ...IDLE, error: INTERRUPTED });
    // ⌘⇧R pressed with the window in the background: no button press here to
    // clear it, but the recording that follows must not sit under a banner
    // about the last one.
    push(STARTING);
    expect(useRecordingStore.getState().error).toBeNull();
    push(RECORDING);
    expect(useRecordingStore.getState().error).toBeNull();
    stop();
  });

  it("tells a window opened after the recording ended why it ended", async () => {
    recordingStatus.mockResolvedValue({ ...IDLE, error: INTERRUPTED });
    await useRecordingStore.getState().refresh();
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);
  });

  it("does not bring back a banner the user dismissed when the status is read again", async () => {
    // Rust keeps the reason on the idle status until the next start, so a
    // reload or a second subscription reads the same error back.
    const ended = { ...IDLE, error: INTERRUPTED };
    recordingStatus.mockResolvedValue(ended);
    const stop = watchRecordingState();
    push(ended);
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);

    useRecordingStore.getState().clearError();
    await useRecordingStore.getState().refresh();
    push(ended);
    expect(useRecordingStore.getState().error).toBeNull();

    // A reload forgets module state; the dismissal survives it.
    useRecordingStore.setState({ status: IDLE, error: null });
    await useRecordingStore.getState().refresh();
    expect(useRecordingStore.getState().error).toBeNull();
    stop();
  });

  it("shows the same reason again once a new recording has ended with it", () => {
    const stop = watchRecordingState();
    push({ ...IDLE, error: INTERRUPTED });
    useRecordingStore.getState().clearError();

    push(STARTING);
    push(RECORDING);
    push({ ...IDLE, error: INTERRUPTED });
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);
    stop();
  });

  it("applies the status a toggle returns the same way as a pushed one", async () => {
    useRecordingStore.setState({ error: INTERRUPTED });
    toggleRecording.mockResolvedValue(STARTING);
    await useRecordingStore.getState().toggle();
    expect(useRecordingStore.getState().status).toEqual(STARTING);
    expect(useRecordingStore.getState().error).toBeNull();

    // Rust never returns an error-carrying status from a toggle that went
    // through today, but if it does, it is shown rather than dropped.
    toggleRecording.mockResolvedValue({ ...IDLE, error: INTERRUPTED });
    await useRecordingStore.getState().toggle();
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);
  });

  it("pauses a live recording and resumes a paused one (TUR-146)", async () => {
    const paused = { ...RECORDING, pause: { pausedAtMs: 5_000, pausedTotalMs: 0 } };
    useRecordingStore.setState({ status: RECORDING });
    pauseRecording.mockResolvedValue(paused);
    await useRecordingStore.getState().togglePause();
    expect(pauseRecording).toHaveBeenCalledOnce();
    expect(useRecordingStore.getState().status).toEqual(paused);

    const resumed = { ...RECORDING, pause: { pausedAtMs: null, pausedTotalMs: 3_000 } };
    resumeRecording.mockResolvedValue(resumed);
    await useRecordingStore.getState().togglePause();
    expect(resumeRecording).toHaveBeenCalledOnce();
    expect(useRecordingStore.getState().status).toEqual(resumed);
    expect(useRecordingStore.getState().busy).toBe(false);
  });

  it("only pauses a live recording", async () => {
    for (const status of [IDLE, STARTING, { ...RECORDING, phase: "stopping" as const }]) {
      useRecordingStore.setState({ status });
      await useRecordingStore.getState().togglePause();
    }
    expect(pauseRecording).not.toHaveBeenCalled();
    expect(resumeRecording).not.toHaveBeenCalled();
  });

  it("shows a refused pause in the banner, and stop goes through the store", async () => {
    useRecordingStore.setState({ status: RECORDING });
    pauseRecording.mockRejectedValue(INTERRUPTED);
    await useRecordingStore.getState().togglePause();
    expect(useRecordingStore.getState().error).toEqual(INTERRUPTED);

    stopRecording.mockResolvedValue(IDLE);
    await useRecordingStore.getState().stop();
    expect(stopRecording).toHaveBeenCalledOnce();
    expect(useRecordingStore.getState().status).toEqual(IDLE);
  });
});
