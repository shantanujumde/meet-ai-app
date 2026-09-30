import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type {
  LiveLine,
  LiveTranscriptSnapshot,
  TranscriptStatus,
  TranscriptUpdate,
} from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";
import {
  applySnapshot,
  applyStatus,
  applyUpdate,
  EMPTY_LIVE,
  type LiveTranscript,
  useTranscriptStore,
  watchLiveTranscript,
} from "./transcript";

/**
 * The live transcript's folding rules, then the store that drives them.
 *
 * The IPC layer is mocked so the store tests can fire events and resolve the
 * snapshot by hand, in whichever order the race under test needs.
 */

let pushUpdate: (update: TranscriptUpdate) => void = () => {};
let pushStatus: (status: TranscriptStatus) => void = () => {};
const liveTranscript = vi.fn<() => Promise<LiveTranscriptSnapshot>>();

vi.mock("@/ipc/client", () => ({
  liveTranscript: () => liveTranscript(),
  onTranscriptUpdate: (handler: (update: TranscriptUpdate) => void) => {
    pushUpdate = handler;
    return () => {
      pushUpdate = () => {};
    };
  },
  onTranscriptStatus: (handler: (status: TranscriptStatus) => void) => {
    pushStatus = handler;
    return () => {
      pushStatus = () => {};
    };
  },
  recordingStatus: vi.fn(),
  toggleRecording: vi.fn(),
  onRecordingState: () => () => {},
}));

const line = (seq: number, speaker: LiveLine["speaker"], text: string, start = seq): LiveLine => ({
  seq,
  speaker,
  start_sec: start,
  text,
});

const volatile = (seq: number, speaker: LiveLine["speaker"], text: string): TranscriptUpdate => ({
  kind: "volatile",
  ...line(seq, speaker, text),
});

const final = (seq: number, speaker: LiveLine["speaker"], text: string): TranscriptUpdate => ({
  kind: "final",
  ...line(seq, speaker, text),
});

const dropped = (seq: number, speaker: LiveLine["speaker"]): TranscriptUpdate => ({
  kind: "dropped",
  seq,
  speaker,
});

function fold(...updates: TranscriptUpdate[]): LiveTranscript {
  return updates.reduce(applyUpdate, EMPTY_LIVE);
}

const texts = (state: LiveTranscript) => state.finals.map((held) => held.text);

describe("applyUpdate", () => {
  test("a volatile guess replaces that speaker's previous guess, and only theirs", () => {
    const state = fold(
      volatile(1, "you", "Sessions are"),
      volatile(2, "others", "Morning"),
      volatile(3, "you", "Sessions are still in memory"),
    );
    expect(state.volatile.you?.text).toBe("Sessions are still in memory");
    expect(state.volatile.others?.text).toBe("Morning");
    expect(state.finals).toEqual([]);
  });

  test("a final clears that speaker's guess and appends the line", () => {
    const state = fold(
      volatile(1, "you", "Sessions are"),
      volatile(2, "others", "Morning"),
      final(3, "you", "Sessions are still in memory."),
    );
    expect(state.volatile.you).toBeNull();
    // The other speaker is still mid-sentence and keeps their guess.
    expect(state.volatile.others?.text).toBe("Morning");
    expect(state.finals).toEqual([line(3, "you", "Sessions are still in memory.")]);
  });

  test("a final carrying the same seq as its guess settles it", () => {
    // Whichever way Rust numbers them — one seq per event, or one per
    // utterance — the guess must not outlive the line that replaced it.
    const state = fold(volatile(4, "others", "Let's start"), final(4, "others", "Let's start."));
    expect(state.volatile.others).toBeNull();
    expect(texts(state)).toEqual(["Let's start."]);
  });

  test("dropped clears the guess and adds nothing", () => {
    const state = fold(volatile(1, "others", "uh"), dropped(2, "others"));
    expect(state.volatile.others).toBeNull();
    expect(state.finals).toEqual([]);
  });

  test("a guess older than the one on screen is ignored", () => {
    const state = fold(volatile(5, "you", "newer"), volatile(3, "you", "older"));
    expect(state.volatile.you?.text).toBe("newer");
  });

  test("a late guess for an utterance that already settled does not come back", () => {
    const state = fold(
      volatile(1, "you", "Sessions"),
      final(2, "you", "Sessions are in memory."),
      volatile(1, "you", "Sessions are"),
    );
    expect(state.volatile.you).toBeNull();
  });

  test("a final older than a newer guess keeps the newer guess", () => {
    // Speaker has already started the next sentence by the time the previous
    // one's final arrives out of order.
    const state = fold(volatile(7, "you", "And the next"), final(6, "you", "That was one."));
    expect(state.volatile.you?.text).toBe("And the next");
    expect(texts(state)).toEqual(["That was one."]);
  });

  test("a late final is slotted in by seq rather than lost", () => {
    const state = fold(
      final(1, "others", "one"),
      final(3, "others", "three"),
      final(2, "you", "two"),
    );
    expect(state.finals.map((held) => held.seq)).toEqual([1, 2, 3]);
  });

  test("a repeated final is not shown twice", () => {
    const state = fold(
      final(1, "you", "hello"),
      final(2, "others", "hi"),
      final(1, "you", "hello"),
    );
    expect(texts(state)).toEqual(["hello", "hi"]);
  });

  test("blank text never becomes a row", () => {
    const state = fold(
      volatile(1, "you", "Hi"),
      volatile(2, "you", "   "),
      final(3, "others", " "),
    );
    expect(state.volatile.you).toBeNull();
    expect(state.finals).toEqual([]);
  });
});

describe("applySnapshot", () => {
  const snapshot = (partial: Partial<LiveTranscriptSnapshot>): LiveTranscriptSnapshot => ({
    status: { state: "running", engine: "apple-speech", detail: null },
    finals: [],
    volatile: [],
    ...partial,
  });

  test("a window opened mid-meeting catches up from the snapshot", () => {
    const state = applySnapshot(
      EMPTY_LIVE,
      snapshot({
        finals: [line(1, "others", "Morning."), line(2, "you", "Hi.")],
        volatile: [line(3, "others", "Let's")],
      }),
    );
    expect(texts(state)).toEqual(["Morning.", "Hi."]);
    expect(state.volatile.others?.text).toBe("Let's");
    expect(state.status.state).toBe("running");
  });

  test("events that landed before the snapshot merge with it without duplicates", () => {
    // Events 2 and 3 arrived while the snapshot (read at seq 2) was in flight.
    const early = fold(final(2, "you", "Hi."), volatile(3, "others", "Let's start"));
    const state = applySnapshot(
      early,
      snapshot({
        finals: [line(1, "others", "Morning."), line(2, "you", "Hi.")],
        volatile: [line(1, "others", "Morn")],
      }),
    );
    expect(state.finals.map((held) => held.seq)).toEqual([1, 2]);
    // The snapshot's guess is older than both the settled line and the newer
    // live guess, so neither is disturbed.
    expect(state.volatile.others?.text).toBe("Let's start");
  });

  test("a status event that beat the snapshot is not overwritten by it", () => {
    const failed = applyStatus(EMPTY_LIVE, {
      state: "failed",
      engine: "whisper",
      detail: "The speech engine quit.",
    });
    const state = applySnapshot(failed, snapshot({}));
    expect(state.status.state).toBe("failed");
  });
});

describe("watchLiveTranscript", () => {
  let stop: () => void = () => {};

  beforeEach(() => {
    useTranscriptStore.setState({ live: EMPTY_LIVE });
    useRecordingStore.setState({
      status: { phase: "idle", meetingId: null, startedAtMs: null, error: null },
    });
    liveTranscript.mockResolvedValue({
      status: { state: "idle", engine: null, detail: null },
      finals: [],
      volatile: [],
    });
  });

  afterEach(() => stop());

  const live = () => useTranscriptStore.getState().live;
  const startRecording = (meetingId: string) =>
    useRecordingStore.setState({
      status: { phase: "recording", meetingId, startedAtMs: 0, error: null },
    });

  test("events fold into the store", () => {
    stop = watchLiveTranscript();
    pushUpdate(final(1, "you", "Hello."));
    pushStatus({ state: "running", engine: "whisper", detail: null });
    expect(texts(live())).toEqual(["Hello."]);
    expect(live().status.engine).toBe("whisper");
  });

  test("a new meeting starts from an empty pane, then catches up", async () => {
    stop = watchLiveTranscript();
    startRecording("meeting-a");
    pushUpdate(final(1, "you", "From the first meeting."));

    liveTranscript.mockResolvedValue({
      status: { state: "running", engine: "apple-speech", detail: null },
      finals: [line(0, "others", "From the second meeting.")],
      volatile: [],
    });
    startRecording("meeting-b");
    expect(live().finals).toEqual([]);

    await vi.waitFor(() => expect(texts(live())).toEqual(["From the second meeting."]));
  });

  test("a snapshot asked for the previous meeting does not land in the next one", async () => {
    let resolveStale: (snapshot: LiveTranscriptSnapshot) => void = () => {};
    liveTranscript.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveStale = resolve;
      }),
    );
    stop = watchLiveTranscript();
    startRecording("meeting-b");

    resolveStale({
      status: { state: "running", engine: null, detail: null },
      finals: [line(9, "you", "Stale, from before the reset.")],
      volatile: [],
    });
    await Promise.resolve();
    await Promise.resolve();
    expect(live().finals).toEqual([]);
  });
});
