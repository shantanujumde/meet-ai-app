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
import {
  isPaused,
  onRecordingState,
  pauseRecording,
  recordingStatus,
  resumeRecording,
  stopRecording,
  toggleRecording,
} from "@/ipc/client";
import type { RecordingStatus, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

const IDLE: RecordingStatus = {
  phase: "idle",
  meetingId: null,
  startedAtMs: null,
  pause: { pausedAtMs: null, pausedTotalMs: 0 },
  error: null,
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
  /** Pause a live recording, or resume a paused one (TUR-146). */
  togglePause: () => Promise<void>;
  /** Stop a live or paused recording: the overlay's Stop (TUR-146). */
  stop: () => Promise<void>;
  clearError: () => void;
  /** Apply a state pushed from Rust. Not for components to call. */
  applyFromBackend: (status: RecordingStatus) => void;
};

/**
 * How many state events this window has heard (TUR-170). Every change Rust
 * makes arrives as an event, in order, so the events are the truth; a
 * command's answer is applied only if no event arrived while it was on its
 * way, or a slow answer (a `starting` read just before `recording` landed)
 * would put the window back on an older state, with no later event to fix it
 * until Stop.
 */
let heard = 0;

/** `status`, unless an event arrived since `asked` (a `heard` count). */
function answer(status: RecordingStatus, asked: number): Partial<RecordingStore> {
  return asked === heard ? withError(status) : {};
}

export const useRecordingStore = create<RecordingStore>((set, get) => ({
  status: IDLE,
  error: null,
  busy: false,

  async refresh() {
    const asked = heard;
    try {
      set(answer(await recordingStatus(), asked));
    } catch (thrown) {
      set({ error: toUiError(thrown) });
    }
  },

  toggle() {
    return request(toggleRecording);
  },

  togglePause() {
    const { status } = get();
    if (status.phase !== "recording") return Promise.resolve();
    return request(isPaused(status) ? resumeRecording : pauseRecording);
  },

  stop() {
    return request(stopRecording);
  },

  clearError() {
    const { error } = get();
    if (error) rememberDismissed(error);
    set({ error: null });
  },

  applyFromBackend(status) {
    heard += 1;
    set(withError(status));
  },
}));

/**
 * One recorder command from this window: never two at once (the buttons
 * disable while `busy`), and its answer or its refusal shown the same way.
 */
async function request(command: () => Promise<RecordingStatus>): Promise<void> {
  const { busy } = useRecordingStore.getState();
  if (busy) return;
  useRecordingStore.setState({ busy: true, error: null });
  const asked = heard;
  try {
    useRecordingStore.setState(answer(await command(), asked));
  } catch (thrown) {
    useRecordingStore.setState({ error: toUiError(thrown) });
  } finally {
    useRecordingStore.setState({ busy: false });
  }
}

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
  if (status.phase === "starting") {
    rememberDismissed(null);
    return { status, error: null };
  }
  if (status.error && !wasDismissed(status.error)) return { status, error: status.error };
  return { status };
}

/**
 * The error the user last dismissed, so it does not come back.
 *
 * Rust keeps the reason on the idle status until the next start — it has to,
 * so a window opened later still learns why — which means every re-read of
 * that status (a webview reload, `watchRecordingState` subscribing again) hands
 * back the banner the user already closed. Remembering what was dismissed, and
 * forgetting it on the next `starting`, keeps it closed without a Rust round
 * trip. Session storage rather than module state because a reload is one of the
 * re-reads; it is a per-window convenience, so a storage that throws or comes
 * back empty only means the banner shows once more.
 */
const DISMISSED_KEY = "meet-ai.recording.dismissed-error";

function errorKey(error: UiError): string {
  return JSON.stringify([error.domain, error.kind, error.message]);
}

function rememberDismissed(error: UiError | null) {
  try {
    if (error) sessionStorage.setItem(DISMISSED_KEY, errorKey(error));
    else sessionStorage.removeItem(DISMISSED_KEY);
  } catch {
    // No storage: the banner may come back once. Nothing else depends on it.
  }
}

function wasDismissed(error: UiError): boolean {
  try {
    return sessionStorage.getItem(DISMISSED_KEY) === errorKey(error);
  } catch {
    return false;
  }
}

/**
 * Start mirroring Rust's recording state.
 *
 * Called once from the app root rather than from a component that might mount
 * twice. Returns the teardown so React StrictMode's double-invoke in
 * development does not leave two listeners running.
 */
export function watchRecordingState(): () => void {
  // A recording Rust stopped by itself (TUR-97), or a ⌘⇧R press it refused
  // (TUR-127), rides in on this same event as an idle status with `error`
  // set, and uses the same banner as a refused button press. Listening
  // first, then reading (TUR-170): a change between the two is heard.
  const stop = onRecordingState((status) => {
    useRecordingStore.getState().applyFromBackend(status);
  });
  void useRecordingStore.getState().refresh();
  return stop;
}
