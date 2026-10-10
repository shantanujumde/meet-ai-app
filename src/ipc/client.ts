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

import { DEFAULT_ROOT_LABEL, NO_MEETINGS_TODAY } from "@/lib/constants";
import { writable } from "@/lib/writable";
import {
  AGENT_RUN_STATUS_EVENT,
  commands,
  DEFAULT_AGENT_CHOICE,
  DEFAULT_ENGINE_ENVIRONMENT,
  DETECTION_PROMPT_EVENT,
  HOOK_FAILED_EVENT,
  MEETINGS_CHANGED_EVENT,
  MODEL_PROGRESS_EVENT,
  type meet_ai_lib_calendar_TodayEvent,
  type meet_ai_lib_calendar_TodaysMeetings,
  type meet_ai_lib_detection_notify_Prompt,
  type meet_ai_lib_hooks_app_HookFailed,
  PERMISSION_STATUS_EVENT,
  RECORDING_STATE_EVENT,
  TRANSCRIPT_STATUS_EVENT,
  TRANSCRIPT_UPDATE_EVENT,
} from "./bindings";
import { NO_BACKEND } from "./errors";
import { subscribe } from "./events";
import type {
  AgentChoice,
  AgentCli,
  AgentTestResult,
  EnvironmentView,
  LiveTranscriptSnapshot,
  MeetingDetail,
  MeetingList,
  MeetingNotes,
  MeetingsChanged,
  ModelProgress,
  ModelView,
  NotesRunStatus,
  OnboardingState,
  PermissionStatus,
  PrivacyPane,
  RecordingStatus,
  SearchHit,
  SelectionView,
  TranscriptStatus,
  TranscriptUpdate,
} from "./types";
import { toUiError } from "./types";

/**
 * Tauri event names. They are generated from `src-tauri/src/events.rs` into
 * `bindings.ts` (rule R2), and re-exported here so callers keep one import.
 *
 * `RECORDING_STATE_EVENT` carries every recorder transition. A recording Rust
 * ended on its own (TUR-97), or a ⌘⇧R or menu-bar press it refused (TUR-127),
 * arrives on it too, as an idle status with `error` set — there is no separate
 * error event.
 */
export {
  AGENT_RUN_STATUS_EVENT,
  DETECTION_PROMPT_EVENT,
  HOOK_FAILED_EVENT,
  MEETINGS_CHANGED_EVENT,
  MODEL_PROGRESS_EVENT,
  PERMISSION_STATUS_EVENT,
  RECORDING_STATE_EVENT,
  TRANSCRIPT_STATUS_EVENT,
  TRANSCRIPT_UPDATE_EVENT,
};

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

/** What the generated bindings return for a command that can fail. */
type Result<G> = { status: "ok"; data: G } | { status: "error"; error: unknown };

/**
 * Call a generated command, normalising whatever it throws into a `UiError`.
 *
 * The generated functions (`./bindings`) fix each command's name, argument
 * names and return type, and `./types` only renames the generated types, so a
 * Rust change that breaks a caller fails `pnpm typecheck` (TUR-173).
 * Commands that return `Result<_, UiError>` on the Rust side come back as a
 * result object; the rest resolve to the bare value.
 */
async function call<G>(run: () => Promise<G | Result<G>>): Promise<G> {
  if (!hasBackend()) throw NO_BACKEND;
  try {
    const answer = await run();
    if (isResult<G>(answer)) {
      if (answer.status === "error") throw answer.error;
      return answer.data;
    }
    return answer;
  } catch (thrown) {
    throw toUiError(thrown);
  }
}

function isResult<G>(value: G | Result<G>): value is Result<G> {
  if (typeof value !== "object" || value === null) return false;
  const status = (value as { status?: unknown }).status;
  return (status === "ok" && "data" in value) || (status === "error" && "error" in value);
}

// --- meetings -------------------------------------------------------------

export async function listMeetings(): Promise<MeetingList> {
  if (!hasBackend()) {
    // The honest empty answer: no backend means no meetings are readable, and
    // the empty state is a designed screen rather than a failure.
    return { root: DEFAULT_ROOT_LABEL, rootExists: false, meetings: [] };
  }
  return call(() => commands.listMeetings());
}

export async function search(query: string): Promise<SearchHit[]> {
  if (!hasBackend()) return [];
  return call(() => commands.search(query));
}

export function readMeeting(id: string): Promise<MeetingDetail> {
  return call(() => commands.readMeeting(id));
}

export async function saveNotes(id: string, body: string): Promise<void> {
  await call(() => commands.saveNotes(id, body));
}

/**
 * Move the meetings folder somewhere else. Every existing meeting moves with
 * it — nothing is left behind at the old location.
 */
export function changeMeetingsFolder(newRoot: string): Promise<MeetingList> {
  return call(() => commands.changeMeetingsFolder(newRoot));
}

export async function revealMeeting(id: string): Promise<void> {
  await call(() => commands.revealMeeting(id));
}

/**
 * The wrap-up prompt for one meeting, in its clipboard form (A11's fallback):
 * the agent it is pasted into writes `meeting.md` and the tickets itself, and
 * the folder watcher picks them up.
 */
export function wrapUpPrompt(meetingId: string): Promise<string> {
  return call(() => commands.wrapUpPrompt(meetingId));
}

/**
 * Whether the user chose no agent (`agent.harness` is `none`), so a meeting is
 * wrapped up by copying its prompt. Whether a chosen agent's CLI can be found
 * is a separate question this does not answer — see `showsCopyPrompt`.
 * Without a backend there is nothing to copy a prompt for, so the answer is no.
 */
export async function copyPromptFallback(): Promise<boolean> {
  if (!hasBackend()) return false;
  return call(() => commands.copyPromptFallback());
}

// --- notes run (TUR-10) ---------------------------------------------------

/**
 * Where this meeting's notes run is. Rust starts one on its own when a
 * recording stops; `idle` means none since launch. Without a backend nothing
 * ever runs, so the answer is idle.
 */
export async function notesRunStatus(meetingId: string): Promise<NotesRunStatus> {
  if (!hasBackend()) return { meetingId, state: { state: "idle" } };
  return call(() => commands.notesRunStatus(meetingId));
}

/**
 * Start the notes run by hand: Retry, or the first run on a meeting with none.
 * Rust ignores it while one is running and answers with that run.
 */
export function startNotesRun(meetingId: string): Promise<NotesRunStatus> {
  return call(() => commands.startNotesRun(meetingId));
}

/** Cancel this meeting's running notes run. A no-op when none is running. */
export function cancelNotesRun(meetingId: string): Promise<NotesRunStatus> {
  return call(() => commands.cancelNotesRun(meetingId));
}

/**
 * The "Make notes for this meeting" switch (TUR-12, SPEC A11). Off writes
 * `agent_notes: off` into `meeting.md`, so no run sends this transcript
 * anywhere, and cancels a run already going. Answers with the notes as they
 * now read.
 */
export function setMeetingNotes(meetingId: string, on: boolean): Promise<MeetingNotes> {
  return call(() => commands.setMeetingNotes(meetingId, on));
}

/** The agent-written sections of this meeting's `meeting.md`. */
export async function meetingNotes(meetingId: string): Promise<MeetingNotes> {
  if (!hasBackend()) return { notesOff: false, analyzedBy: null, sections: [] };
  return call(() => commands.meetingNotes(meetingId));
}

// --- agent setup ----------------------------------------------------------

/**
 * The agent the user picked to write notes (`agent` in config.jsonc). Rejects
 * with `app/unknown-harness` when the file names an agent meet-ai does not
 * know. Without a backend, the default: Claude Code, on the model Claude Code
 * picks itself (blank model).
 */
export async function agentChoice(): Promise<AgentChoice> {
  if (!hasBackend()) return writable(DEFAULT_AGENT_CHOICE);
  return call(() => commands.agentChoice());
}

/**
 * Look for Claude Code and Codex: installed, signed in, which version. Always
 * both, Claude Code first. Takes seconds. `choice.binaryPath` is used only for
 * the agent `choice` names. Without a backend nothing can be found.
 */
export async function detectAgents(choice: AgentChoice): Promise<AgentCli[]> {
  if (!hasBackend()) return [];
  return call(() => commands.detectAgents(choice));
}

/** Save the agent, model and path. Resolves to what was saved, read back from disk. */
export function saveAgentChoice(choice: AgentChoice): Promise<AgentChoice> {
  return call(() => commands.saveAgentChoice(choice));
}

/**
 * Run a 3-line sample transcript through the real notes run, end to end.
 * Can take up to a minute.
 */
export function testAgent(choice: AgentChoice): Promise<AgentTestResult> {
  return call(() => commands.testAgent(choice));
}

// --- permission and onboarding -------------------------------------------

/** Both permission calls' answer when there is no Rust side to ask. */
const NO_BACKEND_PERMISSION: PermissionStatus = {
  state: "unknown",
  measured: false,
  detail: NO_BACKEND.message,
  denied: [],
};

export async function measurePermission(): Promise<PermissionStatus> {
  if (!hasBackend()) return NO_BACKEND_PERMISSION;
  return call(() => commands.measurePermission());
}

/**
 * The silent check the app runs on launch: the microphone's stored decision
 * only, so no chime. It never answers `granted` — see `permission::quick`.
 */
export async function permissionQuick(): Promise<PermissionStatus> {
  if (!hasBackend()) return NO_BACKEND_PERMISSION;
  return call(() => commands.permissionQuick());
}

/**
 * The full check Rust runs when a recording starts, however it was started
 * (window, ⌘⇧R, menu bar). The window's permission state follows it.
 */
export function onPermissionStatus(handler: (status: PermissionStatus) => void): () => void {
  return subscribe(PERMISSION_STATUS_EVENT, handler);
}

export async function openPrivacySettings(pane: PrivacyPane): Promise<void> {
  await call(() => commands.openPrivacySettings(pane));
}

export async function onboardingState(): Promise<OnboardingState> {
  if (!hasBackend()) return { completedAt: null };
  return call(() => commands.onboardingState());
}

export function completeOnboarding(): Promise<OnboardingState> {
  return call(() => commands.completeOnboarding());
}

export function resetOnboarding(): Promise<OnboardingState> {
  return call(() => commands.resetOnboarding());
}

// --- engine and models ----------------------------------------------------

/** Filesystem-only and sub-millisecond. Safe to await before first paint. */
export async function engineEnvironment(): Promise<EnvironmentView> {
  if (!hasBackend()) return writable(DEFAULT_ENGINE_ENVIRONMENT);
  return call(() => commands.engineEnvironment());
}

/**
 * Runs `meet-stt --probe`, median ~160 ms.
 *
 * Never await this before rendering the settings screen — that is the whole
 * reason it is a separate command from {@link engineEnvironment}.
 */
export function engineSelection(): Promise<SelectionView> {
  return call(() => commands.engineSelection());
}

export async function modelCatalogue(): Promise<ModelView[]> {
  if (!hasBackend()) return [];
  return call(() => commands.modelCatalogue());
}

export function downloadModel(id: string): Promise<string> {
  return call(() => commands.downloadModel(id));
}

// --- recording ------------------------------------------------------------

export async function recordingStatus(): Promise<RecordingStatus> {
  if (!hasBackend()) {
    return {
      phase: "idle",
      meetingId: null,
      startedAtMs: null,
      pause: { pausedAtMs: null, pausedTotalMs: 0 },
      error: null,
    };
  }
  return call(() => commands.recordingStatus());
}

export function toggleRecording(): Promise<RecordingStatus> {
  return call(() => commands.toggleRecording());
}

export function stopRecording(): Promise<RecordingStatus> {
  return call(() => commands.stopRecording());
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
  return call(() => commands.liveTranscript());
}

// --- events ---------------------------------------------------------------

export function onRecordingState(handler: (status: RecordingStatus) => void): () => void {
  return subscribe(RECORDING_STATE_EVENT, handler);
}

export function onModelProgress(handler: (progress: ModelProgress) => void): () => void {
  return subscribe(MODEL_PROGRESS_EVENT, handler);
}

export function onTranscriptUpdate(handler: (update: TranscriptUpdate) => void): () => void {
  return subscribe(TRANSCRIPT_UPDATE_EVENT, handler);
}

export function onTranscriptStatus(handler: (status: TranscriptStatus) => void): () => void {
  return subscribe(TRANSCRIPT_STATUS_EVENT, handler);
}

export function onMeetingsChanged(handler: (change: MeetingsChanged) => void): () => void {
  return subscribe(MEETINGS_CHANGED_EVENT, handler);
}

/** A user hook failed (TUR-63), for every meeting: filter by `meetingId`. */
export type HookFailed = meet_ai_lib_hooks_app_HookFailed;

export function onHookFailed(handler: (failed: HookFailed) => void): () => void {
  return subscribe(HOOK_FAILED_EVENT, handler);
}

/** Every notes-run change, for every meeting — filter by `meetingId`. */
export function onNotesRunStatus(handler: (status: NotesRunStatus) => void): () => void {
  return subscribe(AGENT_RUN_STATUS_EVENT, handler);
}

/**
 * A meeting looks like it started and meet-ai is asking whether to record it
 * (TUR-27): what was noticed (`signal`), and why, as one sentence (`reason`).
 */
export type DetectionPrompt = meet_ai_lib_detection_notify_Prompt;

export function onDetectionPrompt(handler: (prompt: DetectionPrompt) => void): () => void {
  return subscribe(DETECTION_PROMPT_EVENT, handler);
}

// --- today's meetings (TUR-28) ----------------------------------------------

/** Today's calendar events, plus how often to re-read them and the solo cut-off. */
export type TodaysMeetings = meet_ai_lib_calendar_TodaysMeetings;
export type TodayEvent = meet_ai_lib_calendar_TodayEvent;

/**
 * Today's events from Calendar.app, local midnight to midnight. Rejects with
 * kind `calendar-denied` when macOS has not granted calendar access — never
 * an empty list. The first call can wait for the macOS prompt. Without a
 * backend there is no calendar, so no events.
 */
export async function todaysMeetings(): Promise<TodaysMeetings> {
  if (!hasBackend()) return NO_MEETINGS_TODAY;
  return call(() => commands.todaysMeetings());
}

/**
 * `calendar.refresh_minutes` without reading the calendar, for when
 * {@link todaysMeetings} failed and so did not say.
 */
export async function calendarRefreshMinutes(): Promise<number> {
  if (!hasBackend()) return NO_MEETINGS_TODAY.refreshMinutes;
  return call(() => commands.calendarRefreshMinutes());
}

// --- command groups in their own modules ------------------------------------

// Cancel on the agent Test screen (TUR-168).
export * from "./agentTest";
// Settings' Light / Dark / System picker and glass switch (TUR-102).
export * from "./appearance";
export * from "./brief";
export * from "./calendar";
// TUR-155: a bad value in config.jsonc, for the Settings cards.
export * from "./configProblem";
// Closing and quitting (TUR-76). Re-exported, so every caller (and
// `@/test/ipcMock`) keeps the one `@/ipc/client` import; `call` and
// `subscribe` are exported for such modules.
export * from "./events";
export * from "./headphones";
export * from "./lifecycle";
export * from "./logs";
export * from "./meetingActions";
export * from "./meetingsWatch";
// Settings' Bluetooth mic switch (TUR-91), then deleting a model (TUR-132).
export * from "./micSetting";
export * from "./models";
export * from "./notesAutoRun";
export * from "./notifications";
export * from "./overlay";
export * from "./recordingPause";
export * from "./retention";
export * from "./settingsSnapshot";
export * from "./speech";
// Tickets, suggested tasks and sending them to the tracker (TUR-113).
export * from "./tickets";
export { call, subscribe };
