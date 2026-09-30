import { act, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type {
  MeetingDetail,
  MeetingList,
  MeetingSummary,
  PermissionStatus,
  RecordingStatus,
} from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";
import { App } from "./App";

/**
 * The shell, driven against a stubbed IPC layer.
 *
 * jsdom is not Tauri, so `src/ipc/client.ts` is mocked rather than left to its
 * own no-backend fallbacks — that way each test states the backend answer it is
 * testing against instead of quietly inheriting a default.
 */

const listMeetings = vi.fn<() => Promise<MeetingList>>();
const permissionStatus = vi.fn<() => Promise<PermissionStatus>>();
const onboardingState = vi.fn();
const readMeeting = vi.fn<(id: string) => Promise<MeetingDetail>>();

vi.mock("@/ipc/client", () => ({
  hasBackend: () => true,
  listMeetings: () => listMeetings(),
  permissionStatus: () => permissionStatus(),
  onboardingState: () => onboardingState(),
  completeOnboarding: vi.fn(),
  resetOnboarding: vi.fn(),
  recordingStatus: vi.fn().mockResolvedValue({
    phase: "idle",
    meetingId: null,
    startedAtMs: null,
  }),
  toggleRecording: vi.fn(),
  openPrivacySettings: vi.fn(),
  revealMeeting: vi.fn(),
  readMeeting: (id: string) => readMeeting(id),
  saveNotes: vi.fn(),
  changeMeetingsFolder: vi.fn(),
  engineEnvironment: vi.fn().mockResolvedValue({
    sidecar: null,
    whisperModel: null,
    locale: "en-US",
    modelId: "large-v3-turbo-q5_0",
    modelsDir: null,
  }),
  engineSelection: vi.fn().mockResolvedValue({ engine: "apple-speech", reason: "built in" }),
  modelCatalogue: vi.fn().mockResolvedValue([]),
  downloadModel: vi.fn(),
  liveTranscript: vi.fn().mockResolvedValue({
    status: { state: "idle", engine: null, detail: null },
    finals: [],
    volatile: [],
  }),
  onRecordingState: () => () => {},
  onRecordingError: () => () => {},
  onModelProgress: () => () => {},
  onTranscriptUpdate: () => () => {},
  onTranscriptStatus: () => () => {},
}));

const EMPTY_LIST: MeetingList = { root: "/Users/test/Meetings", rootExists: false, meetings: [] };

beforeEach(() => {
  window.location.hash = "";
  // The store is module state, so one test's recording must not leak into the next.
  useRecordingStore.setState({
    status: { phase: "idle", meetingId: null, startedAtMs: null },
    error: null,
    busy: false,
  });
  listMeetings.mockResolvedValue(EMPTY_LIST);
  permissionStatus.mockResolvedValue({
    state: "unknown",
    measured: false,
    detail: "not checked yet",
  });
  onboardingState.mockResolvedValue({ completedAt: null });
});

test("a first-time user lands on onboarding rather than on an empty list", async () => {
  render(<App />);
  expect(
    await screen.findByRole("heading", { name: /meet-ai records your meetings/i }),
  ).toBeInTheDocument();
});

test("someone who has finished onboarding gets the meeting list, with its empty state", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);

  const heading = await screen.findByRole("heading", { name: /no meetings yet/i });
  // An empty state with no next step is exactly what the interface bar rules
  // out, so the copy and the button are part of the contract. Scoped to the
  // empty state, because the titlebar carries the same shortcut hint.
  const emptyState = heading.parentElement;
  expect(emptyState).not.toBeNull();
  if (!emptyState) return;
  expect(emptyState.textContent).toMatch(/⌘⇧R/);
  expect(emptyState.textContent).toMatch(/nothing leaves this Mac/i);
  expect(within(emptyState).getByRole("button", { name: /start recording/i })).toBeEnabled();
});

test("someone who has finished onboarding is not trapped on a leftover setup URL", async () => {
  // The single-instance window can be refocused sitting wherever it was left,
  // including partway through setup from before it was finished. Landing back
  // there every time it is reopened is exactly the bug this guards against.
  window.location.hash = "#/onboarding/permission";
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);

  expect(await screen.findByRole("heading", { name: /no meetings yet/i })).toBeInTheDocument();
  expect(
    screen.queryByRole("heading", { name: /let meet-ai hear your mac/i }),
  ).not.toBeInTheDocument();
});

test("a denied permission disables recording instead of letting it fail at click time", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  permissionStatus.mockResolvedValue({
    state: "denied",
    measured: true,
    detail: "the user said no",
  });

  render(<App />);

  const record = await screen.findByRole("button", {
    name: /recording is unavailable because meet-ai is not allowed/i,
  });
  expect(record).toBeDisabled();

  // And the reason is on screen, with a route to fixing it.
  await waitFor(() => {
    expect(screen.getByText(/cannot record this Mac's audio yet/i)).toBeInTheDocument();
  });
  expect(screen.getByRole("button", { name: /fix this/i })).toBeInTheDocument();
});

test("a meeting list that fails to load says so, verbatim, and offers a way on", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  listMeetings.mockRejectedValue({
    domain: "app",
    kind: "io",
    message: "Permission denied (os error 13)",
  });

  render(<App />);

  // The generic fallback: a human sentence, plus the raw message — never a
  // bare error code on its own.
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  expect(screen.getByText(/Permission denied \(os error 13\)/)).toBeInTheDocument();
  expect(screen.getByText(/does not have specific advice/i)).toBeInTheDocument();
});

// --- a recording opens its own meeting ------------------------------------

/** What Rust pushes on `recording://state`, applied as the listener would. */
function pushRecording(phase: RecordingStatus["phase"], meetingId: string | null) {
  const status: RecordingStatus = {
    phase,
    meetingId,
    startedAtMs: meetingId === null ? null : 1_790_000_000_000,
  };
  act(() => {
    useRecordingStore.getState().applyFromBackend(status);
  });
}

/** Render the app for a user who is past onboarding, and wait for it to settle. */
async function renderFinishedApp() {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);
  await screen.findByRole("heading", { name: /no meetings yet/i });
}

const FIRST = "2026-09-30 10-00-00";
const SECOND = "2026-09-30 11-30-00";
const hashFor = (id: string) => `#/meetings/${encodeURIComponent(id)}`;

test("starting a recording opens that meeting, so the live transcript is on screen", async () => {
  await renderFinishedApp();

  // `starting` carries no id yet, so there is nothing to open.
  pushRecording("starting", null);
  expect(window.location.hash).toBe("#/meetings");

  pushRecording("recording", FIRST);
  await waitFor(() => expect(window.location.hash).toBe(hashFor(FIRST)));
});

test("a user who leaves the meeting mid-recording is not pulled back to it", async () => {
  await renderFinishedApp();
  pushRecording("starting", null);
  pushRecording("recording", FIRST);
  await waitFor(() => expect(window.location.hash).toBe(hashFor(FIRST)));

  act(() => {
    window.location.hash = "#/settings";
  });
  await screen.findByRole("heading", { level: 1, name: "Settings" });

  // Further events for the same meeting — a repeat, then the stop — leave the
  // user where they went.
  pushRecording("recording", FIRST);
  pushRecording("stopping", FIRST);
  pushRecording("idle", null);
  expect(window.location.hash).toBe("#/settings");
});

test("a second recording opens the new meeting", async () => {
  await renderFinishedApp();
  pushRecording("starting", null);
  pushRecording("recording", FIRST);
  await waitFor(() => expect(window.location.hash).toBe(hashFor(FIRST)));
  pushRecording("stopping", FIRST);
  pushRecording("idle", null);

  act(() => {
    window.location.hash = "#/meetings";
  });
  pushRecording("starting", null);
  pushRecording("recording", SECOND);
  await waitFor(() => expect(window.location.hash).toBe(hashFor(SECOND)));
});

test("a recording started by the shortcut during onboarding does not pull the user out", async () => {
  // Rust does not gate ⌘⇧R on onboarding, so this can really happen.
  render(<App />);
  await screen.findByRole("heading", { name: /meet-ai records your meetings/i });
  const before = window.location.hash;
  expect(before.startsWith("#/onboarding")).toBe(true);

  pushRecording("starting", null);
  pushRecording("recording", FIRST);
  expect(window.location.hash).toBe(before);
});

// --- TUR-97: a recording killed mid-meeting --------------------------------

const INTERRUPTED: MeetingSummary = {
  id: "2026-09-30-1140-meeting",
  title: "Meeting",
  date: "2026-09-30",
  time: "11:40",
  lineCount: 1,
  lastTimestamp: "00:00:04",
  hasNotes: false,
  hasAnalysis: false,
  recordingState: "interrupted",
  audioMs: 16_253,
};

const FINISHED: MeetingSummary = {
  ...INTERRUPTED,
  id: "2026-09-30-1129-meeting",
  time: "11:29",
  recordingState: "finished",
  audioMs: 55_615,
};

test("an interrupted meeting is labelled in the list, and a finished one is not", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  listMeetings.mockResolvedValue({
    root: "/Users/test/Meetings",
    rootExists: true,
    meetings: [INTERRUPTED, FINISHED],
  });

  render(<App />);

  // Once in the sidebar, once on the list page — and only on its own row.
  const labels = await screen.findAllByText("Interrupted");
  expect(labels).toHaveLength(2);
  for (const label of labels) {
    const row = label.closest("button");
    expect(row?.textContent).toMatch(/11:40/);
    expect(row?.textContent).not.toMatch(/11:29/);
  }
});

test("an interrupted meeting opens like any other and says how much audio was kept", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  listMeetings.mockResolvedValue({
    root: "/Users/test/Meetings",
    rootExists: true,
    meetings: [INTERRUPTED],
  });
  readMeeting.mockResolvedValue({
    summary: INTERRUPTED,
    path: "/Users/test/Meetings/2026-09-30-1140-meeting",
    lines: [{ seq: 0, time: "00:00:04", speaker: "You", text: "Can everyone hear me?" }],
    transcriptMissing: false,
    unparsedLineCount: 0,
    notes: "",
  });
  window.location.hash = "#/meetings/2026-09-30-1140-meeting";

  render(<App />);

  expect(
    await screen.findByText("Recording stopped unexpectedly. Audio up to 00:00:16 was saved."),
  ).toBeInTheDocument();
  // The transcript that survived is right there, and nothing is an error.
  expect(screen.getByText("Can everyone hear me?")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(readMeeting).toHaveBeenCalledWith("2026-09-30-1140-meeting");
});
