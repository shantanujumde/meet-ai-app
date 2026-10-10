import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RecordingStatus, UiError } from "@/ipc/types";

/**
 * TUR-178: the detection prompt's Record, Join and record and Join go
 * through the store's own actions, never by setting `busy` or `error` from
 * outside.
 */

const toggleRecording = vi.fn<() => Promise<RecordingStatus>>();
const recordRemindedMeeting = vi.fn<(eventId: string, join: boolean) => Promise<void>>();
const joinRemindedMeeting = vi.fn<(eventId: string) => Promise<void>>();

vi.mock("@/ipc/client", async () => ({
  isPaused: (await import("@/lib/elapsed")).isPaused,
  toggleRecording: () => toggleRecording(),
  recordRemindedMeeting: (eventId: string, join: boolean) => recordRemindedMeeting(eventId, join),
  joinRemindedMeeting: (eventId: string) => joinRemindedMeeting(eventId),
}));

const { useRecordingStore } = await import("./recording");

const IDLE: RecordingStatus = {
  phase: "idle",
  meetingId: null,
  startedAtMs: null,
  pause: { pausedAtMs: null, pausedTotalMs: 0 },
  error: null,
};

const RECORDING: RecordingStatus = { ...IDLE, phase: "recording", meetingId: "m", startedAtMs: 1 };

const REFUSED: UiError = { domain: "app", kind: "already-recording", message: "Busy." };

beforeEach(() => {
  toggleRecording.mockReset();
  recordRemindedMeeting.mockReset();
  joinRemindedMeeting.mockReset();
  useRecordingStore.setState({ status: IDLE, error: null, busy: false });
});

describe("recordReminded", () => {
  it("starts a recording named after the event, busy while it runs", async () => {
    let finish: () => void = () => {};
    recordRemindedMeeting.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const done = useRecordingStore.getState().recordReminded("standup-1", true);
    expect(useRecordingStore.getState().busy).toBe(true);
    expect(recordRemindedMeeting).toHaveBeenCalledWith("standup-1", true);
    finish();
    await done;
    expect(useRecordingStore.getState().busy).toBe(false);
    // The answer carries no status: the state events bring the recording.
    expect(useRecordingStore.getState().status).toEqual(IDLE);
  });

  it("without an event is a plain start", async () => {
    toggleRecording.mockResolvedValue({ ...IDLE, phase: "starting" });
    await useRecordingStore.getState().recordReminded(null, false);
    expect(toggleRecording).toHaveBeenCalledTimes(1);
    expect(recordRemindedMeeting).not.toHaveBeenCalled();
  });

  it("never stops a recording that is already running", async () => {
    useRecordingStore.setState({ status: RECORDING });
    await useRecordingStore.getState().recordReminded(null, false);
    await useRecordingStore.getState().recordReminded("standup-1", false);
    expect(toggleRecording).not.toHaveBeenCalled();
    expect(recordRemindedMeeting).not.toHaveBeenCalled();
  });

  it("a refusal shows where the record button's would", async () => {
    recordRemindedMeeting.mockRejectedValue(REFUSED);
    await useRecordingStore.getState().recordReminded("standup-1", false);
    expect(useRecordingStore.getState().error).toEqual(REFUSED);
    expect(useRecordingStore.getState().busy).toBe(false);
  });

  it("does nothing while another request is in flight", async () => {
    useRecordingStore.setState({ busy: true });
    await useRecordingStore.getState().recordReminded("standup-1", false);
    expect(recordRemindedMeeting).not.toHaveBeenCalled();
  });
});

describe("joinReminded", () => {
  it("opens the link, and a refusal shows as the store's error", async () => {
    joinRemindedMeeting.mockRejectedValue(REFUSED);
    await useRecordingStore.getState().joinReminded("standup-1");
    expect(joinRemindedMeeting).toHaveBeenCalledWith("standup-1");
    expect(useRecordingStore.getState().error).toEqual(REFUSED);
  });
});
