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
import { DEFAULT_ROOT_LABEL } from "@/lib/constants";
import { NO_BACKEND } from "./errors";
import type {
  EnvironmentView,
  LiveTranscriptSnapshot,
  MeetingDetail,
  MeetingList,
  MeetingsChanged,
  ModelProgress,
  ModelView,
  OnboardingState,
  PermissionStatus,
  PrivacyPane,
  RecordingStatus,
  SelectionView,
  TicketSummary,
  TranscriptStatus,
  TranscriptUpdate,
} from "./types";
import { toUiError } from "./types";

/**
 * Tauri event names. These are string literals shared with Rust.
 *
 * `RECORDING_STATE_EVENT` carries every recorder transition. A recording Rust
 * ended on its own (TUR-97), or a ⌘⇧R or menu-bar press it refused (TUR-127),
 * arrives on it too, as an idle status with `error` set — there is no separate
 * error event.
 */
export const RECORDING_STATE_EVENT = "recording://state";
export const MODEL_PROGRESS_EVENT = "model://progress";
export const PERMISSION_STATUS_EVENT = "permission://status";
export const TRANSCRIPT_UPDATE_EVENT = "transcript://update";
export const TRANSCRIPT_STATUS_EVENT = "transcript://status";
export const MEETINGS_CHANGED_EVENT = "meetings-changed";

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
    return { root: DEFAULT_ROOT_LABEL, rootExists: false, meetings: [] };
  }
  return call<MeetingList>("list_meetings");
}

// --- tickets --------------------------------------------------------------

export async function listTickets(): Promise<TicketSummary[]> {
  if (!hasBackend()) return [];
  return call<TicketSummary[]>("list_tickets");
}

export function createTicket(title: string, body: string): Promise<TicketSummary> {
  return call<TicketSummary>("create_ticket", { title, body });
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

/** Both permission calls' answer when there is no Rust side to ask. */
const NO_BACKEND_PERMISSION: PermissionStatus = {
  state: "unknown",
  measured: false,
  detail: NO_BACKEND.message,
  denied: [],
};

export async function permissionStatus(): Promise<PermissionStatus> {
  if (!hasBackend()) return NO_BACKEND_PERMISSION;
  return call<PermissionStatus>("permission_status");
}

/**
 * The silent check the app runs on launch: the microphone's stored decision
 * only, so no chime. It never answers `granted` — see `permission::quick`.
 */
export async function permissionQuick(): Promise<PermissionStatus> {
  if (!hasBackend()) return NO_BACKEND_PERMISSION;
  return call<PermissionStatus>("permission_quick");
}

/**
 * The full check Rust runs when a recording starts, however it was started
 * (window, ⌘⇧R, menu bar). The window's permission state follows it.
 */
export function onPermissionStatus(handler: (status: PermissionStatus) => void): () => void {
  return subscribe<PermissionStatus>(PERMISSION_STATUS_EVENT, handler);
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
    return { phase: "idle", meetingId: null, startedAtMs: null, error: null };
  }
  return call<RecordingStatus>("recording_status");
}

export function toggleRecording(): Promise<RecordingStatus> {
  return call<RecordingStatus>("toggle_recording");
}

export function stopRecording(): Promise<RecordingStatus> {
  return call<RecordingStatus>("stop_recording");
}

// --- live transcript ------------------------------------------------------

/**
 * The live pane so far: settled lines, each speaker's in-progress guess, and
 * whether transcription is running. Events only carry what changed after they
 * were subscribed to, so this is how a window opened mid-meeting catches up.
 */
export async function liveTranscript(): Promise<LiveTranscriptSnapshot> {
  if (!hasBackend()) {
    return { status: { state: "idle", engine: null, detail: null }, finals: [], volatile: [] };
  }
  return call<LiveTranscriptSnapshot>("live_transcript");
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

export function onModelProgress(handler: (progress: ModelProgress) => void): () => void {
  return subscribe<ModelProgress>(MODEL_PROGRESS_EVENT, handler);
}

export function onTranscriptUpdate(handler: (update: TranscriptUpdate) => void): () => void {
  return subscribe<TranscriptUpdate>(TRANSCRIPT_UPDATE_EVENT, handler);
}

export function onTranscriptStatus(handler: (status: TranscriptStatus) => void): () => void {
  return subscribe<TranscriptStatus>(TRANSCRIPT_STATUS_EVENT, handler);
}

export function onMeetingsChanged(handler: (change: MeetingsChanged) => void): () => void {
  return subscribe<MeetingsChanged>(MEETINGS_CHANGED_EVENT, handler);
}
