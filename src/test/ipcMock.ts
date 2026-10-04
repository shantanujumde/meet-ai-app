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
 * `emit(RECORDING_STATE_EVENT, status)` to push an event the way Rust would
 * (`emit(AGENT_RUN_STATUS_EVENT, status)` for a notes run).
 * `setup.ts` resets every default and drops every listener before each test.
 *
 * The event names are the real ones from `client.ts` (the module's own
 * constants are passed through), so a test cannot fire an event nothing
 * listens to by misspelling it.
 */

import { vi } from "vitest";
import type * as Client from "@/ipc/client";
import type {
  AgentCli,
  AgentModel,
  EngineChoices,
  LiveTranscriptSnapshot,
  MeetingList,
  MeetingNotes,
  NotesRunFailure,
  PermissionStatus,
  RecordingStatus,
} from "@/ipc/types";
import { NO_MEETINGS_TODAY } from "@/lib/constants";
import { meetingDetail, meetingSummary, ticketSummary } from "./fixtures";

const IDLE: RecordingStatus = { phase: "idle", meetingId: null, startedAtMs: null, error: null };

const NOT_CHECKED: PermissionStatus = {
  state: "unknown",
  measured: false,
  detail: "not checked yet",
  denied: [],
};

const DEFAULT_SOURCES: Client.CalendarSources = {
  calendarAppAvailable: true,
  calendarApp: true,
  configured: [],
  connected: [],
};

const SIGNED_OUT: Client.CalendarAccount[] = [
  { provider: "google", account: null, state: "signed-out", remembered: true },
  { provider: "microsoft", account: null, state: "signed-out", remembered: true },
];

const EMPTY_LIST: MeetingList = { root: "/Users/test/Meetings", rootExists: false, meetings: [] };

const NO_NOTES: MeetingNotes = { notesOff: false, analyzedBy: null, sections: [] };

const APPLE_READY: EngineChoices = {
  engine: "auto",
  model: "large-v3-turbo-q5_0",
  auto: "apple-speech",
  apple: { available: true, reason: null },
  whisper: { available: false, reason: "Download a model first." },
  parakeet: { available: false, reason: "Download the Parakeet model first." },
  parakeetModel: {
    id: "parakeet-tdt-0.6b-v3",
    displayName: "Parakeet (25 European languages)",
    goodFor: "Fast on a computer without a graphics card, so live captions keep up.",
    bytes: 670_479_942,
    installed: false,
    runtimeReady: true,
    languages: ["English", "German"],
  },
  languages: ["en-US"],
};

const CANCELLED: NotesRunFailure = {
  kind: "cancelled",
  message: "You cancelled the notes run. Retry to start it again.",
  command: null,
};

const EMPTY_SNAPSHOT: LiveTranscriptSnapshot = {
  status: { state: "idle", engine: null, detail: null },
  finals: [],
  volatile: [],
};

/** Claude Code's models as `crates/agent/models.json` lists them (TUR-74). */
export const CLAUDE_MODELS: AgentModel[] = [
  { name: "sonnet", label: "Sonnet", note: "balanced, good default for notes" },
  { name: "haiku", label: "Haiku", note: "fastest, cheapest" },
  { name: "opus", label: "Opus", note: "most capable, slowest" },
  { name: "claude-sonnet-5-5", label: "claude-sonnet-5-5", note: null },
  { name: "claude-opus-5-5", label: "claude-opus-5-5", note: null },
  { name: "claude-haiku-4-5", label: "claude-haiku-4-5", note: null },
  { name: "claude-fable-5-1", label: "claude-fable-5-1", note: null },
];

/** What agent detection finds by default: Claude Code signed in, Codex not installed. */
const DETECTED_AGENTS: AgentCli[] = [
  {
    id: "claude-code",
    name: "Claude Code",
    provider: "Anthropic",
    state: "ready",
    path: "/usr/local/bin/claude",
    version: "2.1.286",
    signInCommand: "claude auth login",
    models: CLAUDE_MODELS,
    cliDefault: null,
    canTest: true,
  },
  {
    id: "codex",
    name: "Codex",
    provider: "OpenAI",
    state: "missing",
    path: null,
    version: null,
    signInCommand: "codex login",
    models: [],
    cliDefault: null,
    canTest: false,
  },
];

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
  openLogsFolder: vi.fn<typeof Client.openLogsFolder>(async () => {}),
  wrapUpPrompt: vi.fn<typeof Client.wrapUpPrompt>(async (id) => `Wrap up ${id}`),
  copyPromptFallback: vi.fn<typeof Client.copyPromptFallback>(async () => false),

  // No run since launch and nothing written yet: the state a meeting opened
  // after a relaunch is in. A test of a run pushes its statuses with `emit`.
  notesRunStatus: vi.fn<typeof Client.notesRunStatus>(async (meetingId) => ({
    meetingId,
    state: { state: "idle" },
  })),
  startNotesRun: vi.fn<typeof Client.startNotesRun>(async (meetingId) => ({
    meetingId,
    state: { state: "running" },
  })),
  cancelNotesRun: vi.fn<typeof Client.cancelNotesRun>(async (meetingId) => ({
    meetingId,
    state: { state: "failed", failure: CANCELLED },
  })),
  meetingNotes: vi.fn<typeof Client.meetingNotes>(async () => NO_NOTES),
  // The switch answers with the notes as they now read: none written yet,
  // and switched the way it was asked.
  setMeetingNotes: vi.fn<typeof Client.setMeetingNotes>(async (_meetingId, on) => ({
    ...NO_NOTES,
    notesOff: !on,
  })),

  search: vi.fn<typeof Client.search>(async () => []),

  // No earlier meeting with this title (TUR-32).
  meetingBrief: vi.fn<typeof Client.meetingBrief>(async (title) => ({
    title,
    previous: null,
    commits: null,
  })),

  listTickets: vi.fn<typeof Client.listTickets>(async () => []),
  createTicket: vi.fn<typeof Client.createTicket>(async (title, body) => ({
    id: "TUR-1",
    title,
    status: "open",
    meeting: null,
    body,
    hasProblems: false,
    syncedTo: null,
    externalId: null,
    externalUrl: null,
  })),
  startWorkPrompt: vi.fn<typeof Client.startWorkPrompt>(async (id) => `Start work on ${id}`),

  // Tracker sync (TUR-11): a meeting with no tickets, and a sync that works.
  meetingTasks: vi.fn<typeof Client.meetingTasks>(async () => []),
  syncTask: vi.fn<typeof Client.syncTask>(async (id, meeting) =>
    ticketSummary({
      id,
      meeting,
      syncedTo: "linear",
      externalId: "ENG-1",
      externalUrl: "https://linear.app/team/issue/ENG-1",
    }),
  ),
  cancelSync: vi.fn<typeof Client.cancelSync>(async () => {}),
  dismissUnsavedSync: vi.fn<typeof Client.dismissUnsavedSync>(async () => {}),
  openSyncedIssue: vi.fn<typeof Client.openSyncedIssue>(async () => {}),
  trackerSettings: vi.fn<typeof Client.trackerSettings>(async () => ({
    tracker: "linear",
    trackerMcp: "claude.ai Linear",
    harness: "claude-code",
  })),
  setTracker: vi.fn<typeof Client.setTracker>(async (tracker, trackerMcp) => ({
    tracker,
    trackerMcp,
    harness: "claude-code",
  })),
  trackerServers: vi.fn<typeof Client.trackerServers>(async () => []),

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
  // TUR-75: a Mac on macOS 26 with Apple's engine ready and no model downloaded.
  engineChoices: vi.fn<typeof Client.engineChoices>(async () => APPLE_READY),
  // TUR-62: the credit Settings, About shows.
  modelCredits: vi.fn<typeof Client.modelCredits>(async () => [
    {
      text: "Parakeet speech model: parakeet-tdt-0.6b-v3 by NVIDIA, used under CC-BY-4.0.",
      url: "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3",
    },
  ]),
  setTranscription: vi.fn<typeof Client.setTranscription>(async (engine, model) => ({
    ...APPLE_READY,
    engine,
    model,
  })),

  recordingStatus: vi.fn<typeof Client.recordingStatus>(async () => IDLE),
  toggleRecording: vi.fn<typeof Client.toggleRecording>(async () => IDLE),
  stopRecording: vi.fn<typeof Client.stopRecording>(async () => IDLE),

  liveTranscript: vi.fn<typeof Client.liveTranscript>(async () => EMPTY_SNAPSHOT),

  // TUR-76: the Dock icon goes with the window, and quitting works.
  appSettings: vi.fn<typeof Client.appSettings>(async () => ({ showInDockWhenClosed: false })),
  setShowInDockWhenClosed: vi.fn<typeof Client.setShowInDockWhenClosed>(async (show) => ({
    showInDockWhenClosed: show,
  })),
  confirmQuit: vi.fn<typeof Client.confirmQuit>(async () => {}),
  // TUR-77: no countdown next to the menu-bar icon.
  menuBarCountdown: vi.fn<typeof Client.menuBarCountdown>(async () => false),
  setMenuBarCountdown: vi.fn<typeof Client.setMenuBarCountdown>(async (show) => show),
  builtinMicWithBluetooth: vi.fn<typeof Client.builtinMicWithBluetooth>(async () => true),
  setBuiltinMicWithBluetooth: vi.fn<typeof Client.setBuiltinMicWithBluetooth>(async (on) => on),
  // TUR-101: notes start on their own after a call.
  notesAutoRun: vi.fn<typeof Client.notesAutoRun>(async () => true),
  saveNotesAutoRun: vi.fn<typeof Client.saveNotesAutoRun>(async (on) => on),
  // TUR-58: meet-ai does not start at login until the user says so.
  startAtLogin: vi.fn<typeof Client.startAtLogin>(async () => false),
  setStartAtLogin: vi.fn<typeof Client.setStartAtLogin>(async (enabled) => enabled),

  // TUR-78: the SPEC §3.5 detection defaults, and notifications allowed.
  notificationSettings: vi.fn<typeof Client.notificationSettings>(async () => ({
    remind: true,
    remindBeforeMinutes: 1,
    processes: true,
    audioActivity: true,
    minAttendees: 2,
  })),
  setNotificationSettings: vi.fn<typeof Client.setNotificationSettings>(
    async (settings) => settings,
  ),
  osNotificationsBlocked: vi.fn<typeof Client.osNotificationsBlocked>(async () => false),
  openNotificationSettings: vi.fn<typeof Client.openNotificationSettings>(async () => {}),
  sendTestReminder: vi.fn<typeof Client.sendTestReminder>(async () => true),
  joinRemindedMeeting: vi.fn<typeof Client.joinRemindedMeeting>(async () => {}),
  recordRemindedMeeting: vi.fn<typeof Client.recordRemindedMeeting>(async () => {}),

  // TUR-28: a calendar that was read and has nothing today.
  todaysMeetings: vi.fn<typeof Client.todaysMeetings>(async () => NO_MEETINGS_TODAY),
  calendarRefreshMinutes: vi.fn<typeof Client.calendarRefreshMinutes>(
    async () => NO_MEETINGS_TODAY.refreshMinutes,
  ),

  // TUR-49: a fresh Mac — the Calendar app on, no sign-in set up.
  calendarSources: vi.fn<typeof Client.calendarSources>(async () => DEFAULT_SOURCES),
  calendarAccounts: vi.fn<typeof Client.calendarAccounts>(async () => SIGNED_OUT),
  setCalendarApp: vi.fn<typeof Client.setCalendarApp>(async (on) => ({
    ...DEFAULT_SOURCES,
    calendarApp: on,
  })),
  calendarConnect: vi.fn<typeof Client.calendarConnect>(async (provider) => ({
    provider,
    account: "ada@example.com",
    state: "signed-in",
    remembered: true,
  })),
  calendarDisconnect: vi.fn<typeof Client.calendarDisconnect>(async () => DEFAULT_SOURCES),

  // TUR-45: the SPEC §3.5 default.
  audioRetentionDays: vi.fn<typeof Client.audioRetentionDays>(async () => ({
    state: "running" as const,
    days: 7,
  })),

  agentChoice: vi.fn<typeof Client.agentChoice>(async () => ({
    harness: "claude-code",
    model: "",
    binaryPath: null,
  })),
  detectAgents: vi.fn<typeof Client.detectAgents>(async () => DETECTED_AGENTS),
  saveAgentChoice: vi.fn<typeof Client.saveAgentChoice>(async (choice) => choice),
  testAgent: vi.fn<typeof Client.testAgent>(async () => ({
    summary: "The beta ships Friday.",
    decisions: ["Ship the beta Friday"],
    openQuestions: ["Do we need legal sign-off?"],
    tasks: [{ title: "Write the release notes", owner: "Ben", due: "Thursday" }],
    seconds: 9.6,
  })),
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
    onNotesRunStatus: subscriber(actual.AGENT_RUN_STATUS_EVENT),
    onDetectionPrompt: subscriber(actual.DETECTION_PROMPT_EVENT),
    onQuitConfirm: subscriber(actual.QUIT_CONFIRM_EVENT),
    onNavigate: subscriber(actual.NAVIGATE_EVENT),
    onHookFailed: subscriber(actual.HOOK_FAILED_EVENT),
  };
}

/** Back to the defaults, with no listeners. Run before every test by `setup.ts`. */
export function resetIpcMock(): void {
  for (const mock of Object.values(ipc)) mock.mockReset();
  listeners.clear();
}
