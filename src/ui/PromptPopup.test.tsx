import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { DetectionPrompt, PopupAnswer, PopupPrompt } from "@/ipc/client";
import type { RecordingStatus } from "@/ipc/types";
import { PromptPopup } from "./PromptPopup";

const fake = vi.hoisted(() => ({
  answers: [] as [number, string][],
  current: null as unknown,
  onPrompt: null as ((shown: unknown) => void) | null,
  onRecording: null as ((status: unknown) => void) | null,
}));

vi.mock("@/ipc/client", () => ({
  promptPopupCurrent: () => Promise.resolve(fake.current),
  answerPromptPopup: (id: number, answer: PopupAnswer) => {
    fake.answers.push([id, answer]);
    return Promise.resolve();
  },
  onPromptPopup: (handler: (shown: unknown) => void) => {
    fake.onPrompt = handler;
    return () => {};
  },
  onRecordingState: (handler: (status: unknown) => void) => {
    fake.onRecording = handler;
    return () => {};
  },
}));

const ZOOM: DetectionPrompt = {
  signal: { kind: "process", process: "zoom.us" },
  reason: "Zoom is open.",
  updateOnly: false,
  eventId: null,
  canJoin: false,
  test: false,
};

const WITH_LINK: DetectionPrompt = {
  signal: { kind: "calendar", title: "Standup", attendees: 3 },
  reason: "“Standup” starts in 1 min, with 3 people invited.",
  updateOnly: false,
  eventId: "standup-1",
  canJoin: true,
  test: false,
};

async function showPopup(shown: PopupPrompt) {
  render(<PromptPopup />);
  await act(async () => fake.onPrompt?.(shown));
}

function buttons(): string[] {
  return screen.getAllByRole("button").map((button) => button.textContent ?? "");
}

beforeEach(() => {
  fake.answers = [];
  fake.current = null;
});
afterEach(cleanup);

describe("PromptPopup", () => {
  test("shows the reason with Record and Dismiss", async () => {
    await showPopup({ id: 1, prompt: ZOOM });
    expect(screen.getByText("Zoom is open.")).toBeTruthy();
    expect(buttons()).toEqual(["Record", "Dismiss"]);
  });

  test("Record sends one answer for this prompt and closes", async () => {
    await showPopup({ id: 4, prompt: ZOOM });
    await act(async () => fireEvent.click(screen.getByText("Record")));
    expect(fake.answers).toEqual([[4, "record"]]);
    expect(screen.queryByText("Record")).toBeNull();
  });

  test("Dismiss closes it and records nothing", async () => {
    await showPopup({ id: 2, prompt: ZOOM });
    await act(async () => fireEvent.click(screen.getByText("Dismiss")));
    expect(fake.answers).toEqual([[2, "dismiss"]]);
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("a reminder with a link offers Join, which leaves it up", async () => {
    await showPopup({ id: 3, prompt: WITH_LINK });
    expect(buttons()).toEqual(["Join and record", "Join", "Record", "Dismiss"]);
    await act(async () => fireEvent.click(screen.getByText("Join")));
    expect(fake.answers).toEqual([[3, "join"]]);
    expect(screen.getByText("Join and record")).toBeTruthy();
  });

  test("a new prompt replaces the old one", async () => {
    await showPopup({ id: 1, prompt: ZOOM });
    await act(async () => fake.onPrompt?.({ id: 2, prompt: WITH_LINK }));
    expect(screen.queryByText("Zoom is open.")).toBeNull();
    await act(async () => fireEvent.click(screen.getByText("Dismiss")));
    expect(fake.answers).toEqual([[2, "dismiss"]]);
  });

  test("a window that loads late shows the prompt already up", async () => {
    fake.current = { id: 7, prompt: ZOOM };
    await act(async () => {
      render(<PromptPopup />);
    });
    expect(screen.getByText("Zoom is open.")).toBeTruthy();
  });

  test("a recording started elsewhere closes it without recording", async () => {
    await showPopup({ id: 5, prompt: ZOOM });
    const recording: RecordingStatus = {
      phase: "recording",
      meetingId: "m",
      startedAtMs: 1,
      error: null,
    };
    await act(async () => fake.onRecording?.(recording));
    expect(screen.queryByRole("button")).toBeNull();
    expect(fake.answers).toEqual([[5, "dismiss"]]);
  });
});
