/**
 * The live transcript, as the window holds it while a meeting runs.
 *
 * Like the recording store, this is a **mirror**: Rust owns the session and the
 * file, and this only folds its events into something to render. Per SPEC §2.5
 * the pane and `transcript.md` are deliberately not identical mid-meeting — the
 * settled lines match the file, and each speaker's volatile guess lives here in
 * memory only, never on disk.
 *
 * The folding is a set of pure functions so the awkward cases (events out of
 * order, a snapshot racing the events it overlaps) are testable without a
 * window. The rules:
 *
 * * `volatile` replaces that speaker's guess. `final` clears it and adds the
 *   line; `dropped` clears it and adds nothing.
 * * `seq` is meeting-global and monotonic, so it orders everything. A guess
 *   older than one already shown, or older than a line that already settled
 *   that speaker, is late and ignored — otherwise a stale guess would reappear
 *   under the line that replaced it.
 * * Settled lines are never dropped for arriving late: they are on disk, and
 *   the pane must match the file after stop. A late one is slotted in by `seq`;
 *   a repeat of one already held is ignored.
 */

import { create } from "zustand";
import { liveTranscript, onTranscriptStatus, onTranscriptUpdate } from "@/ipc/client";
import type {
  LiveLine,
  LiveSpeaker,
  LiveTranscriptSnapshot,
  TranscriptStatus,
  TranscriptUpdate,
} from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";

export type LiveTranscript = {
  status: TranscriptStatus;
  /** Settled lines, in `seq` order. The same lines `transcript.md` holds. */
  finals: LiveLine[];
  /** Each speaker's in-progress guess, or null while they are not talking. */
  volatile: Record<LiveSpeaker, LiveLine | null>;
  /**
   * The highest `seq` that settled or dropped each speaker's utterance. A
   * volatile at or below it belongs to an utterance that is already over.
   */
  closedThrough: Record<LiveSpeaker, number>;
  /**
   * A status event has landed since the last reset. The snapshot's status was
   * read before that event, so it must not overwrite it.
   */
  statusFromEvent: boolean;
};

export const IDLE_STATUS: TranscriptStatus = { state: "idle", engine: null, detail: null };

export const EMPTY_LIVE: LiveTranscript = {
  status: IDLE_STATUS,
  finals: [],
  volatile: { you: null, others: null },
  closedThrough: { you: -1, others: -1 },
  statusFromEvent: false,
};

/** Fold one `transcript://update` event in. Returns the same object if nothing changed. */
export function applyUpdate(state: LiveTranscript, update: TranscriptUpdate): LiveTranscript {
  const { speaker, seq } = update;
  const guess = state.volatile[speaker];

  // A blank guess is the recognizer clearing its throat. §3.4 never writes
  // empty text, so the pane does not show an empty row for it either — it
  // withdraws the guess, exactly like `dropped`, so an older guess delivered
  // after it cannot reappear.
  if (update.kind === "volatile" && update.text.trim() !== "") {
    if (seq <= state.closedThrough[speaker]) return state;
    if (guess && seq < guess.seq) return state;
    const { kind: _, ...line } = update;
    return { ...state, volatile: { ...state.volatile, [speaker]: line } };
  }

  // `final`, `dropped` and a blank guess all end the utterance. The guess is cleared only if
  // it is not newer than this update — a newer guess is the next utterance.
  const volatile =
    guess && guess.seq <= seq ? { ...state.volatile, [speaker]: null } : state.volatile;
  const closedThrough = {
    ...state.closedThrough,
    [speaker]: Math.max(state.closedThrough[speaker], seq),
  };

  if (update.kind === "dropped" || update.text.trim() === "") {
    return { ...state, volatile, closedThrough };
  }

  const { kind: _, ...line } = update;
  return { ...state, volatile, closedThrough, finals: insertBySeq(state.finals, line) };
}

/** A `transcript://status` event always wins: it is the newest word from Rust. */
export function applyStatus(state: LiveTranscript, status: TranscriptStatus): LiveTranscript {
  return { ...state, status, statusFromEvent: true };
}

/**
 * Merge the `live_transcript` snapshot with whatever events already landed.
 *
 * Events may have arrived while the snapshot was in flight, so the snapshot is
 * replayed through {@link applyUpdate} rather than replacing the state — the
 * same `seq` rules then dedupe the overlap in both directions.
 */
export function applySnapshot(
  state: LiveTranscript,
  snapshot: LiveTranscriptSnapshot,
): LiveTranscript {
  let next = state;
  for (const line of snapshot.finals) next = applyUpdate(next, { kind: "final", ...line });
  for (const line of snapshot.volatile) next = applyUpdate(next, { kind: "volatile", ...line });
  if (!next.statusFromEvent) next = { ...next, status: snapshot.status };
  return next;
}

/** Add a settled line in `seq` order, ignoring one already held. */
function insertBySeq(finals: LiveLine[], line: LiveLine): LiveLine[] {
  const last = finals.at(-1);
  // The common case by far: lines arrive in order and go on the end.
  if (!last || last.seq < line.seq) return [...finals, line];
  if (finals.some((held) => held.seq === line.seq)) return finals;
  const at = finals.findIndex((held) => held.seq > line.seq);
  return [...finals.slice(0, at), line, ...finals.slice(at)];
}

// --- the store ------------------------------------------------------------

type TranscriptStore = {
  live: LiveTranscript;
};

export const useTranscriptStore = create<TranscriptStore>(() => ({ live: EMPTY_LIVE }));

function setLive(fold: (state: LiveTranscript) => LiveTranscript) {
  useTranscriptStore.setState((store) => ({ live: fold(store.live) }));
}

/**
 * Start mirroring the live transcript.
 *
 * Called once from the app root next to {@link watchRecordingState}, so lines
 * keep landing while the user is on another screen. Returns the teardown so
 * StrictMode's double-invoke does not leave two sets of listeners running.
 *
 * `seq` restarts with every meeting, so the pane is emptied whenever a new
 * meeting starts recording and then caught up from the snapshot.
 */
export function watchLiveTranscript(): () => void {
  // Bumped on every reset, so a snapshot requested for the previous meeting
  // cannot land in the next one.
  let generation = 0;

  async function catchUp() {
    const asked = generation;
    try {
      const snapshot = await liveTranscript();
      if (asked === generation) setLive((state) => applySnapshot(state, snapshot));
    } catch {
      // Nothing to catch up from. The events still arrive, which is the
      // correct degraded behaviour — the pane just starts from now.
    }
  }

  const stopUpdates = onTranscriptUpdate((event) => setLive((state) => applyUpdate(state, event)));
  const stopStatus = onTranscriptStatus((status) => setLive((state) => applyStatus(state, status)));

  let meetingId = useRecordingStore.getState().status.meetingId;
  const stopRecording = useRecordingStore.subscribe((store) => {
    const next = store.status.meetingId;
    if (next !== null && next !== meetingId) {
      generation += 1;
      useTranscriptStore.setState({ live: EMPTY_LIVE });
      void catchUp();
    }
    meetingId = next;
  });

  // The window may have opened mid-meeting. Asking costs nothing when idle.
  void catchUp();

  return () => {
    generation += 1;
    stopUpdates();
    stopStatus();
    stopRecording();
  };
}
