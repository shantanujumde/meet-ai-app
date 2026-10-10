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
import {
  DEFAULT_AGENT_CHOICE,
  DEFAULT_APP_SETTINGS,
  DEFAULT_APPEARANCE,
  DEFAULT_AUDIO_RETENTION,
  DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH,
  DEFAULT_CALENDAR_SOURCES,
  DEFAULT_ENGINE_ENVIRONMENT,
  DEFAULT_MENU_BAR_COUNTDOWN,
  DEFAULT_NOTES_AUTO_RUN,
  DEFAULT_NOTIFICATION_SETTINGS,
  DEFAULT_TRACKER_SETTINGS,
} from "@/ipc/bindings";
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
  TrackerSettings,
} from "@/ipc/types";
import { NO_MEETINGS_TODAY } from "@/lib/constants";
import { writable } from "@/lib/writable";
import { meetingDetail, meetingSummary, ticketSummary } from "./fixtures";

const IDLE: RecordingStatus = {
  phase: "idle",
  meetingId: null,
  startedAtMs: null,
  pause: { pausedAtMs: null, pausedTotalMs: 0 },
  error: null,
};

const NOT_CHECKED: PermissionStatus = {
  state: "unknown",
  measured: false,
  detail: "not checked yet",
  denied: [],
};

// TUR-173: the config defaults are Rust's own (`DEFAULT_*` in bindings.ts).
const DEFAULT_SOURCES: Client.CalendarSources = writable(DEFAULT_CALENDAR_SOURCES);

/** The shipped tracker defaults, as if the user had saved them. */
const SAVED_TRACKER: TrackerSettings = { ...DEFAULT_TRACKER_SETTINGS, chosen: true };

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
  spokenLanguage: "auto",
  spokenLanguages: [
    { code: "hinglish", name: "Hinglish (Hindi and English)" },
    { code: "en", name: "English" },
    { code: "hi", name: "Hindi" },
    { code: "mr", name: "Marathi" },
  ],
  configProblem: null,
  selectionError: null,
  honoursLanguage: true,
  languageIgnoredReason: null,
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
  renameMeeting: vi.fn<typeof Client.renameMeeting>(async (_id, title) => title.trim()),
  // TUR-116: the folder went to the Trash.
  deleteMeeting: vi.fn<typeof Client.deleteMeeting>(async () => {}),
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
    suggested: false,
    owner: null,
    due: null,
    meetingTitle: null,
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
  trackerSettings: vi.fn<typeof Client.trackerSettings>(async () => SAVED_TRACKER),
  setTracker: vi.fn<typeof Client.setTracker>(async (tracker, trackerMcp) => ({
    ...SAVED_TRACKER,
    tracker,
    trackerMcp,
  })),
  trackerServers: vi.fn<typeof Client.trackerServers>(async () => []),
  // TUR-113: suggested tasks and sending on their own. No tracker set up.
  approveTask: vi.fn<typeof Client.approveTask>(async (meeting, id) =>
    ticketSummary({ id, meeting, suggested: false }),
  ),
  approveAllTasks: vi.fn<typeof Client.approveAllTasks>(async () => []),
  discardTask: vi.fn<typeof Client.discardTask>(async () => {}),
  ticketSyncStates: vi.fn<typeof Client.ticketSyncStates>(async () => ({
    trackerSetUp: false,
    tracker: "linear",
    tickets: [],
  })),
  retryTicketSync: vi.fn<typeof Client.retryTicketSync>(async () => {}),
  sendTestTicket: vi.fn<typeof Client.sendTestTicket>(async () => ({
    project: "Engineering",
    message: "Claude Code reached Linear. New tickets will go to Engineering.",
  })),

  measurePermission: vi.fn<typeof Client.measurePermission>(async () => NOT_CHECKED),
  permissionQuick: vi.fn<typeof Client.permissionQuick>(async () => NOT_CHECKED),
  openPrivacySettings: vi.fn<typeof Client.openPrivacySettings>(async () => {}),

  onboardingState: vi.fn<typeof Client.onboardingState>(async () => ({ completedAt: null })),
  completeOnboarding: vi.fn<typeof Client.completeOnboarding>(async () => ({
    completedAt: "2026-09-27T13:00:00+05:30",
  })),
  resetOnboarding: vi.fn<typeof Client.resetOnboarding>(async () => ({ completedAt: null })),

  engineEnvironment: vi.fn<typeof Client.engineEnvironment>(async () =>
    writable(DEFAULT_ENGINE_ENVIRONMENT),
  ),
  engineSelection: vi.fn<typeof Client.engineSelection>(async () => ({
    engine: "apple-speech",
    reason: "built in",
  })),
  modelCatalogue: vi.fn<typeof Client.modelCatalogue>(async () => []),
  downloadModel: vi.fn<typeof Client.downloadModel>(async (id) => id),
  deleteModel: vi.fn<typeof Client.deleteModel>(async () => undefined),
  // TUR-75: a Mac on macOS 26 with Apple's engine ready and no model downloaded.
  engineChoices: vi.fn<typeof Client.engineChoices>(async () => APPLE_READY),
  // TUR-62: the credit Settings, About shows.
  modelCredits: vi.fn<typeof Client.modelCredits>(async () => [
    {
      text: "Parakeet speech model: parakeet-tdt-0.6b-v3 by NVIDIA, used under CC-BY-4.0.",
      url: "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3",
    },
  ]),
  setSpokenLanguage: vi.fn<typeof Client.setSpokenLanguage>(async (spokenLanguage) => ({
    ...APPLE_READY,
    spokenLanguage,
  })),
  setTranscription: vi.fn<typeof Client.setTranscription>(async (engine, model) => ({
    ...APPLE_READY,
    engine,
    model,
  })),

  recordingStatus: vi.fn<typeof Client.recordingStatus>(async () => IDLE),
  toggleRecording: vi.fn<typeof Client.toggleRecording>(async () => IDLE),
  stopRecording: vi.fn<typeof Client.stopRecording>(async () => IDLE),
  // TUR-146: pause and resume, and the recording overlay (on by default).
  pauseRecording: vi.fn<typeof Client.pauseRecording>(async () => IDLE),
  resumeRecording: vi.fn<typeof Client.resumeRecording>(async () => IDLE),
  showRecordingOverlay: vi.fn<typeof Client.showRecordingOverlay>(async () => true),
  setShowRecordingOverlay: vi.fn<typeof Client.setShowRecordingOverlay>(async (on) => on),
  overlayShowMain: vi.fn<typeof Client.overlayShowMain>(async () => {}),

  liveTranscript: vi.fn<typeof Client.liveTranscript>(async () => EMPTY_SNAPSHOT),

  // TUR-76: the Dock icon goes with the window, and quitting works.
  appSettings: vi.fn<typeof Client.appSettings>(async () => writable(DEFAULT_APP_SETTINGS)),
  setShowInDockWhenClosed: vi.fn<typeof Client.setShowInDockWhenClosed>(async (show) => ({
    showInDockWhenClosed: show,
  })),
  confirmQuit: vi.fn<typeof Client.confirmQuit>(async () => {}),
  // TUR-77: no countdown next to the menu-bar icon.
  menuBarCountdown: vi.fn<typeof Client.menuBarCountdown>(async () => DEFAULT_MENU_BAR_COUNTDOWN),
  setMenuBarCountdown: vi.fn<typeof Client.setMenuBarCountdown>(async (show) => show),
  builtinMicWithBluetooth: vi.fn<typeof Client.builtinMicWithBluetooth>(
    async () => DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH,
  ),
  setBuiltinMicWithBluetooth: vi.fn<typeof Client.setBuiltinMicWithBluetooth>(async (on) => on),
  // TUR-101: notes start on their own after a call.
  notesAutoRun: vi.fn<typeof Client.notesAutoRun>(async () => DEFAULT_NOTES_AUTO_RUN),
  saveNotesAutoRun: vi.fn<typeof Client.saveNotesAutoRun>(async (on) => on),
  // TUR-102: follow the OS, glass on.
  appearanceSettings: vi.fn<typeof Client.appearanceSettings>(async () =>
    writable(DEFAULT_APPEARANCE),
  ),
  setAppearance: vi.fn<typeof Client.setAppearance>(async (appearance) => appearance),
  // TUR-155: every config.jsonc section valid.
  configProblem: vi.fn<typeof Client.configProblem>(async () => null),
  // TUR-58: meet-ai does not start at login until the user says so.
  startAtLogin: vi.fn<typeof Client.startAtLogin>(async () => false),
  setStartAtLogin: vi.fn<typeof Client.setStartAtLogin>(async (enabled) => enabled),

  // TUR-78: the SPEC §3.5 detection defaults, and notifications allowed.
  notificationSettings: vi.fn<typeof Client.notificationSettings>(async () =>
    writable(DEFAULT_NOTIFICATION_SETTINGS),
  ),
  setNotificationSettings: vi.fn<typeof Client.setNotificationSettings>(
    async (settings) => settings,
  ),
  // TUR-143: no app on the "Never detect" list yet.
  neverDetectApps: vi.fn<typeof Client.neverDetectApps>(async () => []),
  setNeverDetectApps: vi.fn<typeof Client.setNeverDetectApps>(async (apps) => apps),
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
  // TUR-174: a sign-in was waiting, and now is not.
  calendarCancelSignIn: vi.fn<typeof Client.calendarCancelSignIn>(async () => true),

  // TUR-45: the SPEC §3.5 default.
  audioRetentionDays: vi.fn<typeof Client.audioRetentionDays>(async () =>
    writable(DEFAULT_AUDIO_RETENTION),
  ),

  // TUR-65: not read yet, so no banner.
  headphoneWarning: vi.fn<typeof Client.headphoneWarning>(async () => null),
  meetingsWatchProblem: vi.fn<typeof Client.meetingsWatchProblem>(async () => null),

  agentChoice: vi.fn<typeof Client.agentChoice>(async () => writable(DEFAULT_AGENT_CHOICE)),
  detectAgents: vi.fn<typeof Client.detectAgents>(async () => DETECTED_AGENTS),
  saveAgentChoice: vi.fn<typeof Client.saveAgentChoice>(async (choice) => choice),
  testAgent: vi.fn<typeof Client.testAgent>(async () => ({
    summary: "The beta ships Friday.",
    decisions: ["Ship the beta Friday"],
    openQuestions: ["Do we need legal sign-off?"],
    tasks: [{ title: "Write the release notes", owner: "Ben", due: "Thursday" }],
    seconds: 9.6,
  })),
  cancelAgentTest: vi.fn<typeof Client.cancelAgentTest>(async () => {}),

  // TUR-171: Settings' one read, with the same answers as the commands above.
  settingsSnapshot: vi.fn<typeof Client.settingsSnapshot>(async () => ({
    ...writable(DEFAULT_APP_SETTINGS),
    menuBarCountdown: DEFAULT_MENU_BAR_COUNTDOWN,
    appProblem: null,
    appearanceProblem: null,
    detectionProblem: null,
    audioRetention: writable(DEFAULT_AUDIO_RETENTION),
    builtinMicWithBluetooth: DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH,
    showRecordingOverlay: true,
    agentChoice: writable(DEFAULT_AGENT_CHOICE),
    notesAutoRun: DEFAULT_NOTES_AUTO_RUN,
    agentError: null,
    notifications: writable(DEFAULT_NOTIFICATION_SETTINGS),
    calendarSources: DEFAULT_SOURCES,
    tracker: SAVED_TRACKER,
    trackerError: null,
  })),
};

type Handler = (payload: never) => void;

const listeners = new Map<string, Set<Handler>>();

/** How many times each event was subscribed to since the last reset (TUR-171). */
const subscribeCounts = new Map<string, number>();

/** How many times the window subscribed to `event`: once, for a listener kept across renders. */
export function subscribeCount(event: string): number {
  return subscribeCounts.get(event) ?? 0;
}

/** A stand-in for one of `client.ts`'s `on…` subscribers, keyed by event name. */
function subscriber<T>(event: string): (handler: (payload: T) => void) => () => void {
  return (handler) => {
    subscribeCounts.set(event, subscribeCount(event) + 1);
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
    onHeadphoneWarning: subscriber(actual.HEADPHONE_WARNING_EVENT),
    onMeetingsWatchProblem: subscriber(actual.MEETINGS_WATCH_PROBLEM_EVENT),
    onTicketSync: subscriber(actual.TICKET_SYNC_EVENT),
  };
}

/** Back to the defaults, with no listeners. Run before every test by `setup.ts`. */
export function resetIpcMock(): void {
  for (const mock of Object.values(ipc)) mock.mockReset();
  listeners.clear();
  subscribeCounts.clear();
}
