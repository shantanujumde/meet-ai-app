import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import type { MeetingList, PermissionStatus, UiError } from "@/ipc/types";
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
const permissionQuick = vi.fn<() => Promise<PermissionStatus>>();
const onboardingState = vi.fn();
// The window's handler for a ⌘⇧R refusal, captured so a test can fire one.
let refusedHandler: ((error: UiError) => void) | null = null;

vi.mock("@/ipc/client", () => ({
  hasBackend: () => true,
  listMeetings: () => listMeetings(),
  permissionStatus: () => permissionStatus(),
  permissionQuick: () => permissionQuick(),
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
  readMeeting: vi.fn(),
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
  onRecordingState: () => () => {},
  onRecordingRefused: (handler: (error: UiError) => void) => {
    refusedHandler = handler;
    return () => {
      refusedHandler = null;
    };
  },
  onModelProgress: () => () => {},
}));

const EMPTY_LIST: MeetingList = { root: "/Users/test/Meetings", rootExists: false, meetings: [] };

beforeEach(() => {
  window.location.hash = "";
  listMeetings.mockResolvedValue(EMPTY_LIST);
  permissionStatus.mockReset();
  permissionQuick.mockReset();
  permissionStatus.mockResolvedValue({
    state: "unknown",
    measured: false,
    detail: "not checked yet",
  });
  permissionQuick.mockResolvedValue({
    state: "unknown",
    measured: false,
    detail: "not checked yet",
  });
  onboardingState.mockResolvedValue({ completedAt: null });
});

test("opening the app does not play the permission tone", async () => {
  // The full check plays an audible chime (TUR-24). SPEC A7 keeps that to
  // setup and the start of a recording, so launch must use the silent check.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);

  await screen.findByRole("heading", { name: /no meetings yet/i });
  await waitFor(() => expect(permissionQuick).toHaveBeenCalled());
  expect(permissionStatus).not.toHaveBeenCalled();
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
  const denied: PermissionStatus = { state: "denied", measured: true, detail: "the user said no" };
  permissionQuick.mockResolvedValue(denied);
  permissionStatus.mockResolvedValue(denied);

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

test("Fix this opens the permission screen even after onboarding is finished", async () => {
  // The leftover-URL redirect above must not swallow a deliberate trip there:
  // it did, and the button silently bounced straight back to the list.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  const denied: PermissionStatus = { state: "denied", measured: true, detail: "the user said no" };
  permissionQuick.mockResolvedValue(denied);
  permissionStatus.mockResolvedValue(denied);

  render(<App />);

  fireEvent.click(await screen.findByRole("button", { name: /fix this/i }));

  expect(
    await screen.findByRole("heading", { name: /let meet-ai hear your mac/i }),
  ).toBeInTheDocument();
  // Give the redirect effect its chance to run, then check it did not.
  await waitFor(() => expect(onboardingState).toHaveBeenCalled());
  expect(screen.getByRole("heading", { name: /let meet-ai hear your mac/i })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /open microphone/i })).toBeInTheDocument();
});

test("the permission screen links to both Settings panes, not only system audio", async () => {
  // Before any denial there was one button, and it opened System Audio
  // Recording — someone looking for the Microphone switch landed on the wrong
  // pane. Both grants are needed (L5), so both get a way there.
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /get started/i }));

  expect(
    await screen.findByRole("heading", { name: /let meet-ai hear your mac/i }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /open microphone/i })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /open system audio recording/i })).toBeInTheDocument();
});

test("a ⌘⇧R press that is refused says why in the window", async () => {
  // The shortcut runs in Rust with no button to put an error next to. Its only
  // report used to be a system notification, which is silent when meet-ai may
  // not post them — so the press looked like it did nothing at all.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);
  await screen.findByRole("heading", { name: /no meetings yet/i });
  await waitFor(() => expect(refusedHandler).not.toBeNull());

  act(() => {
    refusedHandler?.({
      domain: "app",
      kind: "permission-denied",
      message: "meet-ai is not allowed to record this Mac's audio",
    });
  });

  expect(await screen.findByRole("alert")).toHaveTextContent(
    /not allowed to record this Mac's audio/i,
  );
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
