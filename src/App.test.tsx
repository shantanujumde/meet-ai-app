import { render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { MeetingList, PermissionStatus } from "@/ipc/types";
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
    stub: true,
  }),
  toggleRecording: vi.fn(),
  openPrivacySettings: vi.fn(),
  revealMeeting: vi.fn(),
  readMeeting: vi.fn(),
  saveNotes: vi.fn(),
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
  onRecordingState: () => () => {},
  onModelProgress: () => () => {},
}));

const EMPTY_LIST: MeetingList = { root: "/Users/test/Meetings", rootExists: false, meetings: [] };

beforeEach(() => {
  window.location.hash = "";
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
