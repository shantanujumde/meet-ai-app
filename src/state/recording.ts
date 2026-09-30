/**
 * The recording state, as the window sees it.
 *
 * Rust owns the state machine — the menu bar, ⌘⇧R and the window button all go
 * through one `Recorder` — so this store is a **mirror, not a second source of
 * truth**. It never guesses a transition optimistically: pressing the button
 * calls Rust and waits, and every change (including ones this window did not
 * cause, like the global shortcut firing while the app is in the background)
 * arrives on the `recording://state` event.
 *
 * Getting that backwards is the classic bug here: an optimistic UI that flips
 * to "Recording" and then has to flip back leaves the user unsure whether
 * anything is being captured, which is the one question this app must never be
 * vague about.
 */

import { create } from "zustand";
import { onRecordingState, recordingStatus, toggleRecording } from "@/ipc/client";
import type { RecordingStatus, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

const IDLE: RecordingStatus = {
  phase: "idle",
  meetingId: null,
  startedAtMs: null,
};

type RecordingStore = {
  status: RecordingStatus;
  /**
   * Why the last start or stop was refused, or why a recording ended on its
   * own. Cleared on the next attempt, from this window or anywhere else.
   */
  error: UiError | null;
  /** A request is in flight. The button disables so it cannot be double-fired. */
  busy: boolean;
  refresh: () => Promise<void>;
  toggle: () => Promise<void>;
  clearError: () => void;
  /** Apply a state pushed from Rust. Not for components to call. */
  applyFromBackend: (status: RecordingStatus) => void;
};

export const useRecordingStore = create<RecordingStore>((set, get) => ({
  status: IDLE,
  error: null,
  busy: false,

  async refresh() {
    try {
      set(withError(await recordingStatus()));
    } catch (thrown) {
      set({ error: toUiError(thrown) });
    }
  },

  async toggle() {
    if (get().busy) return;
    set({ busy: true, error: null });
    try {
      set({ status: await toggleRecording() });
    } catch (thrown) {
      set({ error: toUiError(thrown) });
    } finally {
      set({ busy: false });
    }
  },

  clearError() {
    set({ error: null });
  },

  applyFromBackend(status) {
    set(withError(status));
  },
}));

/**
 * A status from Rust, plus what it means for the shown error.
 *
 * A recording that ends on its own (a checkpoint could not be written, a
 * device change could not be followed), or a ⌘⇧R press Rust refused, has no
 * button press to return an error to, so Rust puts the reason on the idle
 * status instead (TUR-97, TUR-127). A `starting` status is a new attempt from
 * somewhere — the button, the menu bar or ⌘⇧R — so the old reason goes, the
 * same moment Rust drops it. Any other status without an error leaves the
 * current one alone: the idle event that follows a refused Stop must not wipe
 * the reason the refusal just showed.
 */
function withError(status: RecordingStatus): Partial<RecordingStore> {
  if (status.error) return { status, error: status.error };
  if (status.phase === "starting") return { status, error: null };
  return { status };
}

/**
 * Start mirroring Rust's recording state.
 *
 * Called once from the app root rather than from a component that might mount
 * twice. Returns the teardown so React StrictMode's double-invoke in
 * development does not leave two listeners running.
 */
export function watchRecordingState(): () => void {
  void useRecordingStore.getState().refresh();
  // A recording Rust stopped by itself (TUR-97), or a ⌘⇧R press it refused
  // (TUR-127), rides in on this same event as an idle status with `error`
  // set, and uses the same banner as a refused button press.
  return onRecordingState((status) => {
    useRecordingStore.getState().applyFromBackend(status);
  });
}
