import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import {
  MEETINGS_CHANGED_EVENT,
  PERMISSION_STATUS_EVENT,
  RECORDING_STATE_EVENT,
} from "@/ipc/client";
import type { PermissionStatus, RecordingStatus, UiError } from "@/ipc/types";
import { useRecordingStore } from "@/state/recording";
import { meetingDetail, meetingSummary, transcriptLine } from "@/test/fixtures";
import { emit, ipc, listening } from "@/test/ipcMock";
import { App } from "./App";

/**
 * The shell, driven against a stubbed IPC layer.
 *
 * jsdom is not Tauri, so `src/ipc/client.ts` is mocked rather than left to its
 * own no-backend fallbacks — that way each test states the backend answer it is
 * testing against instead of quietly inheriting a default. The shared mock's
 * defaults are an empty, not-yet-onboarded Mac; see `src/test/ipcMock.ts`.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { listMeetings, measurePermission, permissionQuick, onboardingState, readMeeting } = ipc;

beforeEach(() => {
  window.location.hash = "";
});

test("opening the app does not play the permission tone", async () => {
  // The full check plays an audible chime (TUR-24). SPEC A7 keeps that to
  // setup and the start of a recording, so launch must use the silent check.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);

  await screen.findByRole("heading", { name: /no meetings yet/i });
  await waitFor(() => expect(permissionQuick).toHaveBeenCalled());
  expect(measurePermission).not.toHaveBeenCalled();
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
  const denied: PermissionStatus = {
    state: "denied",
    measured: true,
    detail: "the user said no",
    denied: ["microphone"],
  };
  permissionQuick.mockResolvedValue(denied);
  measurePermission.mockResolvedValue(denied);

  render(<App />);

  const record = await screen.findByRole("button", {
    name: /recording is unavailable because meet-ai is not allowed/i,
  });
  expect(record).toBeDisabled();

  // And the reason is on screen, naming the switch, with a route to fixing it.
  await waitFor(() => {
    expect(screen.getByText(/Microphone is switched off for meet-ai/i)).toBeInTheDocument();
  });
  expect(screen.getByRole("button", { name: /fix this/i })).toBeInTheDocument();
});

test("Fix this opens the permission screen even after onboarding is finished", async () => {
  // The leftover-URL redirect above must not swallow a deliberate trip there:
  // it did, and the button silently bounced straight back to the list.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  const denied: PermissionStatus = {
    state: "denied",
    measured: true,
    detail: "the user said no",
    denied: ["microphone"],
  };
  permissionQuick.mockResolvedValue(denied);
  measurePermission.mockResolvedValue(denied);

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

test("Fix audio permission first opens the permission screen after onboarding", async () => {
  // The empty meeting list has its own route there, and it bounced back to the
  // list the same way the banner's "Fix this" did.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  const denied: PermissionStatus = {
    state: "denied",
    measured: true,
    detail: "the user said no",
    denied: ["microphone"],
  };
  permissionQuick.mockResolvedValue(denied);
  measurePermission.mockResolvedValue(denied);

  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /fix audio permission first/i }));

  expect(
    await screen.findByRole("heading", { name: /let meet-ai hear your mac/i }),
  ).toBeInTheDocument();
  await waitFor(() => expect(onboardingState).toHaveBeenCalled());
  expect(screen.getByRole("heading", { name: /let meet-ai hear your mac/i })).toBeInTheDocument();
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

test("a refusal at Record time disables recording and names the switch that is off", async () => {
  // Launch only reads the mic's stored decision, so a system-audio denial is
  // first found when Record runs the tone check. The window must follow that
  // answer: SPEC §8.1 keeps the controls disabled while permission is absent.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);
  await screen.findByRole("heading", { name: /no meetings yet/i });
  await waitFor(() => expect(listening(PERMISSION_STATUS_EVENT)).toBe(true));

  act(() => {
    emit(PERMISSION_STATUS_EVENT, {
      state: "denied",
      measured: true,
      detail: "system audio: the check tone did not come back",
      denied: ["audio-capture"],
    } satisfies PermissionStatus);
  });

  expect(
    await screen.findByRole("button", { name: /recording is unavailable because/i }),
  ).toBeDisabled();
  expect(screen.getByRole("status")).toHaveTextContent(/System Audio Recording/);
  expect(screen.getByRole("button", { name: /fix this/i })).toBeInTheDocument();

  // Switched back on in Settings: the next check's grant clears it.
  act(() => {
    emit(PERMISSION_STATUS_EVENT, {
      state: "granted",
      measured: true,
      detail: "both heard",
      denied: [],
    } satisfies PermissionStatus);
  });
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: /fix this/i })).not.toBeInTheDocument(),
  );
});

test("a ⌘⇧R press that is refused says why in the window", async () => {
  // The shortcut runs in Rust with no button to put an error next to. Its only
  // report used to be a system notification, which is silent when meet-ai may
  // not post them — so the press looked like it did nothing at all.
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  render(<App />);
  await screen.findByRole("heading", { name: /no meetings yet/i });
  await waitFor(() => expect(listening(RECORDING_STATE_EVENT)).toBe(true));

  // Rust says it on the idle status the refused start falls back to.
  const refused: UiError = {
    domain: "app",
    kind: "permission-denied",
    message: "meet-ai is not allowed to record this Mac's audio",
  };
  act(() => {
    emit(RECORDING_STATE_EVENT, {
      phase: "idle",
      meetingId: null,
      startedAtMs: null,
      error: refused,
    } satisfies RecordingStatus);
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

// --- a recording opens its own meeting ------------------------------------

/** What Rust pushes on `recording://state`, applied as the listener would. */
function pushRecording(phase: RecordingStatus["phase"], meetingId: string | null) {
  const status: RecordingStatus = {
    phase,
    meetingId,
    startedAtMs: meetingId === null ? null : 1_790_000_000_000,
    error: null,
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

test("the meeting list is re-read when a recording begins and when it ends, not in between", async () => {
  // Each read walks every WAV header in the root, so the in-between phases
  // (`starting`, `stopping`) must not trigger one.
  await renderFinishedApp();
  await waitFor(() => expect(listMeetings).toHaveBeenCalled());
  listMeetings.mockClear();

  pushRecording("starting", null);
  expect(listMeetings).not.toHaveBeenCalled();
  pushRecording("recording", FIRST);
  expect(listMeetings).toHaveBeenCalledTimes(1);
  pushRecording("stopping", FIRST);
  expect(listMeetings).toHaveBeenCalledTimes(1);
  pushRecording("idle", null);
  expect(listMeetings).toHaveBeenCalledTimes(2);
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

const INTERRUPTED = meetingSummary({
  id: "2026-09-30-1140-meeting",
  time: "11:40",
  lastTimestamp: "00:00:04",
  recordingState: "interrupted",
  audioMs: 16_253,
});

const FINISHED = meetingSummary({
  ...INTERRUPTED,
  id: "2026-09-30-1129-meeting",
  time: "11:29",
  recordingState: "finished",
  audioMs: 55_615,
});

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
  readMeeting.mockResolvedValue(
    meetingDetail({
      summary: INTERRUPTED,
      lines: [transcriptLine({ text: "Can everyone hear me?" })],
    }),
  );
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

// --- TUR-12: a meeting with notes switched off -----------------------------

const PRIVATE = meetingSummary({
  id: "2026-09-30-1200-meeting",
  time: "12:00",
  notesOff: true,
});

test("a meeting with notes switched off is marked in the list, and others are not", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  listMeetings.mockResolvedValue({
    root: "/Users/test/Meetings",
    rootExists: true,
    meetings: [PRIVATE, FINISHED],
  });

  render(<App />);

  // Once in the sidebar, once on the list page — and only on its own row.
  const labels = await screen.findAllByText("Notes off");
  expect(labels).toHaveLength(2);
  for (const label of labels) {
    const row = label.closest("button");
    expect(row?.textContent).toMatch(/12:00/);
    expect(row?.textContent).not.toMatch(/11:29/);
  }
});

test("a recording meeting with notes off keeps its recording dot beside the marker", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  listMeetings.mockResolvedValue({
    root: "/Users/test/Meetings",
    rootExists: true,
    meetings: [PRIVATE],
  });
  ipc.recordingStatus.mockResolvedValue({
    phase: "recording",
    meetingId: PRIVATE.id,
    startedAtMs: 0,
    error: null,
  });

  render(<App />);

  const sidebar = await screen.findByRole("navigation", { name: "Meetings" });
  await waitFor(() => expect(within(sidebar).getByText("● Recording")).toBeInTheDocument());
  expect(within(sidebar).getByText("Notes off")).toBeInTheDocument();
});

test("a change made outside the app refreshes the list quietly and leaves the open meeting alone", async () => {
  onboardingState.mockResolvedValue({ completedAt: "2026-09-27T13:00:00+05:30" });
  listMeetings.mockResolvedValue({
    root: "/Users/test/Meetings",
    rootExists: true,
    meetings: [INTERRUPTED],
  });
  readMeeting.mockResolvedValue(
    meetingDetail({ summary: INTERRUPTED, lines: [transcriptLine({ text: "Still here" })] }),
  );
  window.location.hash = "#/meetings/2026-09-30-1140-meeting";
  render(<App />);
  expect(await screen.findByText("Still here")).toBeInTheDocument();
  await waitFor(() => expect(listening(MEETINGS_CHANGED_EVENT)).toBe(true));
  const hash = window.location.hash;
  listMeetings.mockClear();

  act(() => {
    emit(MEETINGS_CHANGED_EVENT, { paths: ["/Users/test/Meetings/new"] });
  });

  await waitFor(() => expect(listMeetings).toHaveBeenCalledTimes(1));
  expect(window.location.hash).toBe(hash);
  expect(screen.getByText("Still here")).toBeInTheDocument();
});
