/**
 * One mock of `@/ipc/client` for component tests, instead of a hand-written
 * `vi.mock` factory per file.
 *
 * jsdom is not Tauri, so a test mocks the client rather than leaving it to its
 * no-backend fallbacks — that way each test states the backend answer it is
 * testing against. What this adds is a *default* for every command (a fresh
 * Mac on first launch: onboarding not finished, no meetings, permission not
 * yet checked, nothing recording), so a test only overrides the answers it is
 * about — a test of the finished app sets `onboardingState` itself.
 *
 * Use it with:
 *
 * ```ts
 * vi.mock("@/ipc/client", async (importOriginal) =>
 *   (await import("@/test/ipcMock")).mockClient(await importOriginal()),
 * );
 * ```
 *
 * then `ipc.listMeetings.mockResolvedValue(…)` to change an answer, and
 * `emit(RECORDING_STATE_EVENT, status)` to push an event the way Rust would.
 * `setup.ts` resets every default and drops every listener before each test.
 *
 * The event names are the real ones from `client.ts` (the module's own
 * constants are passed through), so a test cannot fire an event nothing
 * listens to by misspelling it.
 */

import { vi } from "vitest";
import type * as Client from "@/ipc/client";
import type {
  LiveTranscriptSnapshot,
  MeetingList,
  PermissionStatus,
  RecordingStatus,
} from "@/ipc/types";
import { meetingDetail, meetingSummary } from "./fixtures";

const IDLE: RecordingStatus = { phase: "idle", meetingId: null, startedAtMs: null, error: null };

const NOT_CHECKED: PermissionStatus = {
  state: "unknown",
  measured: false,
  detail: "not checked yet",
  denied: [],
};

const EMPTY_LIST: MeetingList = { root: "/Users/test/Meetings", rootExists: false, meetings: [] };

const EMPTY_SNAPSHOT: LiveTranscriptSnapshot = {
  status: { state: "idle", engine: null, detail: null },
  finals: [],
  volatile: [],
};

/**
 * Every command, as a `vi.fn` whose own implementation is the default answer.
 * `mockReset()` goes back to exactly that, which is what the per-test reset
 * relies on.
 */
export const ipc = {
  hasBackend: vi.fn<typeof Client.hasBackend>(() => true),

  listMeetings: vi.fn<typeof Client.listMeetings>(async () => EMPTY_LIST),
  readMeeting: vi.fn<typeof Client.readMeeting>(async (id) =>
    meetingDetail({ summary: meetingSummary({ id }) }),
  ),
  saveNotes: vi.fn<typeof Client.saveNotes>(async () => {}),
  changeMeetingsFolder: vi.fn<typeof Client.changeMeetingsFolder>(async (root) => ({
    ...EMPTY_LIST,
    root,
  })),
  revealMeeting: vi.fn<typeof Client.revealMeeting>(async () => {}),
  wrapUpPrompt: vi.fn<typeof Client.wrapUpPrompt>(async (id) => `Wrap up ${id}`),
  copyPromptFallback: vi.fn<typeof Client.copyPromptFallback>(async () => false),

  search: vi.fn<typeof Client.search>(async () => []),

  listTickets: vi.fn<typeof Client.listTickets>(async () => []),
  createTicket: vi.fn<typeof Client.createTicket>(async (title, body) => ({
    id: "TUR-1",
    title,
    status: "open",
    meeting: null,
    body,
    hasProblems: false,
  })),
  startWorkPrompt: vi.fn<typeof Client.startWorkPrompt>(async (id) => `Start work on ${id}`),

  measurePermission: vi.fn<typeof Client.measurePermission>(async () => NOT_CHECKED),
  permissionQuick: vi.fn<typeof Client.permissionQuick>(async () => NOT_CHECKED),
  openPrivacySettings: vi.fn<typeof Client.openPrivacySettings>(async () => {}),

  onboardingState: vi.fn<typeof Client.onboardingState>(async () => ({ completedAt: null })),
  completeOnboarding: vi.fn<typeof Client.completeOnboarding>(async () => ({
    completedAt: "2026-09-27T13:00:00+05:30",
  })),
  resetOnboarding: vi.fn<typeof Client.resetOnboarding>(async () => ({ completedAt: null })),

  engineEnvironment: vi.fn<typeof Client.engineEnvironment>(async () => ({
    sidecar: null,
    whisperModel: null,
    locale: "en-US",
    modelId: "large-v3-turbo-q5_0",
    modelsDir: null,
  })),
  engineSelection: vi.fn<typeof Client.engineSelection>(async () => ({
    engine: "apple-speech",
    reason: "built in",
  })),
  modelCatalogue: vi.fn<typeof Client.modelCatalogue>(async () => []),
  downloadModel: vi.fn<typeof Client.downloadModel>(async (id) => id),

  recordingStatus: vi.fn<typeof Client.recordingStatus>(async () => IDLE),
  toggleRecording: vi.fn<typeof Client.toggleRecording>(async () => IDLE),
  stopRecording: vi.fn<typeof Client.stopRecording>(async () => IDLE),

  liveTranscript: vi.fn<typeof Client.liveTranscript>(async () => EMPTY_SNAPSHOT),
};

type Handler = (payload: never) => void;

const listeners = new Map<string, Set<Handler>>();

/** A stand-in for one of `client.ts`'s `on…` subscribers, keyed by event name. */
function subscriber<T>(event: string): (handler: (payload: T) => void) => () => void {
  return (handler) => {
    const handlers = listeners.get(event) ?? new Set<Handler>();
    handlers.add(handler);
    listeners.set(event, handlers);
    return () => {
      handlers.delete(handler);
    };
  };
}

/** Push an event to whatever the window is listening with, as Rust would. */
export function emit(event: string, payload: unknown): void {
  for (const handler of [...(listeners.get(event) ?? [])]) {
    (handler as (payload: unknown) => void)(payload);
  }
}

/** Whether anything is subscribed to `event` yet — to wait for a listener before emitting. */
export function listening(event: string): boolean {
  return (listeners.get(event)?.size ?? 0) > 0;
}

/**
 * The mocked module: the real one's constants, the commands above, and
 * subscribers wired to {@link emit}.
 */
export function mockClient(actual: typeof Client): typeof Client {
  return {
    ...actual,
    ...ipc,
    onRecordingState: subscriber(actual.RECORDING_STATE_EVENT),
    onModelProgress: subscriber(actual.MODEL_PROGRESS_EVENT),
    onPermissionStatus: subscriber(actual.PERMISSION_STATUS_EVENT),
    onTranscriptUpdate: subscriber(actual.TRANSCRIPT_UPDATE_EVENT),
    onTranscriptStatus: subscriber(actual.TRANSCRIPT_STATUS_EVENT),
    onMeetingsChanged: subscriber(actual.MEETINGS_CHANGED_EVENT),
  };
}

/** Back to the defaults, with no listeners. Run before every test by `setup.ts`. */
export function resetIpcMock(): void {
  for (const mock of Object.values(ipc)) mock.mockReset();
  listeners.clear();
}
