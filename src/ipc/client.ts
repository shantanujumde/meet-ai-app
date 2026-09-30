/**
 * Typed wrappers around every `invoke` this app makes.
 *
 * One file, so the IPC surface is countable: this is the whole contract with
 * `src-tauri/src/commands.rs`, and the two are edited together.
 *
 * **Outside Tauri** — `pnpm dev` in a plain browser, or a component test — there
 * is no Rust behind the bridge. Rather than letting `invoke` throw an opaque
 * error from deep inside a screen, every function checks first and either
 * returns the honest empty answer (for reads) or rejects with a `no-backend`
 * {@link UiError} (for anything that would change something). The UI then shows
 * its real empty and error states instead of a blank page, which is exactly
 * what you want when iterating on them.
 */

import { invoke } from "@tauri-apps/api/core";
import { type Event, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  EnvironmentView,
  MeetingDetail,
  MeetingList,
  ModelProgress,
  ModelView,
  OnboardingState,
  PermissionStatus,
  PrivacyPane,
  RecordingStatus,
  SelectionView,
  UiError,
} from "./types";
import { toUiError } from "./types";

/** Tauri event names. These are string literals shared with Rust. */
export const RECORDING_STATE_EVENT = "recording://state";
/**
 * Rust ended a recording on its own because a checkpoint failed (TUR-97). The
 * state event has already moved to idle; this one says why.
 */
export const RECORDING_ERROR_EVENT = "recording://error";
export const MODEL_PROGRESS_EVENT = "model://progress";

/**
 * Is there a Rust side to talk to?
 *
 * Tauri 2 injects `__TAURI_INTERNALS__` into the webview. Checking for it is
 * more reliable than sniffing the user agent or the protocol, both of which
 * change between dev and a bundled app.
 */
export function hasBackend(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const NO_BACKEND: UiError = {
  domain: "app",
  kind: "no-backend",
  message:
    "This is the meet-ai window running without its Mac app behind it, so it cannot read or " +
    "change anything on disk. Run `pnpm tauri dev` instead of `pnpm dev`.",
};

/** Call a Rust command, normalising whatever it throws into a `UiError`. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!hasBackend()) throw NO_BACKEND;
  try {
    return await invoke<T>(command, args);
  } catch (thrown) {
    throw toUiError(thrown);
  }
}

// --- meetings -------------------------------------------------------------

export async function listMeetings(): Promise<MeetingList> {
  if (!hasBackend()) {
    // The honest empty answer: no backend means no meetings are readable, and
    // the empty state is a designed screen rather than a failure.
    return { root: "~/Meetings", rootExists: false, meetings: [] };
  }
  return call<MeetingList>("list_meetings");
}

export function readMeeting(id: string): Promise<MeetingDetail> {
  return call<MeetingDetail>("read_meeting", { id });
}

export function saveNotes(id: string, body: string): Promise<void> {
  return call<void>("save_notes", { id, body });
}

/**
 * Move the meetings folder somewhere else. Every existing meeting moves with
 * it — nothing is left behind at the old location.
 */
export function changeMeetingsFolder(newRoot: string): Promise<MeetingList> {
  return call<MeetingList>("change_meetings_folder", { newRoot });
}

export function revealMeeting(id: string): Promise<void> {
  return call<void>("reveal_meeting", { id });
}

// --- permission and onboarding -------------------------------------------

export async function permissionStatus(): Promise<PermissionStatus> {
  if (!hasBackend()) {
    return {
      state: "unknown",
      measured: false,
      detail: NO_BACKEND.message,
    };
  }
  return call<PermissionStatus>("permission_status");
}

export function openPrivacySettings(pane: PrivacyPane): Promise<void> {
  return call<void>("open_privacy_settings", { pane });
}

export async function onboardingState(): Promise<OnboardingState> {
  if (!hasBackend()) return { completedAt: null };
  return call<OnboardingState>("onboarding_state");
}

export function completeOnboarding(): Promise<OnboardingState> {
  return call<OnboardingState>("complete_onboarding");
}

export function resetOnboarding(): Promise<OnboardingState> {
  return call<OnboardingState>("reset_onboarding");
}

// --- engine and models ----------------------------------------------------

/** Filesystem-only and sub-millisecond. Safe to await before first paint. */
export async function engineEnvironment(): Promise<EnvironmentView> {
  if (!hasBackend()) {
    return {
      sidecar: null,
      whisperModel: null,
      locale: "en-US",
      modelId: "large-v3-turbo-q5_0",
      modelsDir: null,
    };
  }
  return call<EnvironmentView>("engine_environment");
}

/**
 * Runs `meet-stt --probe`, median ~160 ms.
 *
 * Never await this before rendering the settings screen — that is the whole
 * reason it is a separate command from {@link engineEnvironment}.
 */
export function engineSelection(): Promise<SelectionView> {
  return call<SelectionView>("engine_selection");
}

export async function modelCatalogue(): Promise<ModelView[]> {
  if (!hasBackend()) return [];
  return call<ModelView[]>("model_catalogue");
}

export function downloadModel(id: string): Promise<string> {
  return call<string>("download_model", { id });
}

// --- recording ------------------------------------------------------------

export async function recordingStatus(): Promise<RecordingStatus> {
  if (!hasBackend()) {
    return { phase: "idle", meetingId: null, startedAtMs: null };
  }
  return call<RecordingStatus>("recording_status");
}

export function toggleRecording(): Promise<RecordingStatus> {
  return call<RecordingStatus>("toggle_recording");
}

export function stopRecording(): Promise<RecordingStatus> {
  return call<RecordingStatus>("stop_recording");
}

// --- events ---------------------------------------------------------------

/**
 * Subscribe to a Tauri event, returning an unsubscribe function.
 *
 * The `listen` call is async but React effects need a synchronous cleanup, so
 * the returned function tears down whichever of the two wins the race — an
 * effect that unmounts before `listen` resolves must still not leak a listener.
 */
function subscribe<T>(event: string, onEvent: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | undefined;
  let cancelled = false;

  if (hasBackend()) {
    listen<T>(event, (received: Event<T>) => onEvent(received.payload))
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Nothing to listen to. The caller's state stays at whatever its last
        // explicit fetch returned, which is the correct degraded behaviour.
      });
  }

  return () => {
    cancelled = true;
    unlisten?.();
  };
}

export function onRecordingState(handler: (status: RecordingStatus) => void): () => void {
  return subscribe<RecordingStatus>(RECORDING_STATE_EVENT, handler);
}

export function onRecordingError(handler: (error: UiError) => void): () => void {
  return subscribe<UiError>(RECORDING_ERROR_EVENT, handler);
}

export function onModelProgress(handler: (progress: ModelProgress) => void): () => void {
  return subscribe<ModelProgress>(MODEL_PROGRESS_EVENT, handler);
}
