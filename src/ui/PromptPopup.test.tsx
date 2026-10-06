import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { DetectionPrompt } from "@/ipc/client";
import type { PopupAnswer, PopupPrompt } from "@/ipc/promptPopup";
import type { RecordingStatus } from "@/ipc/types";
import { choicesFor, joinWords, PromptPopup } from "./PromptPopup";
import { formatTime } from "./TodayPane";

type MenuItem = { id: string; text: string; action: () => void };

const fake = vi.hoisted(() => ({
  answers: [] as [number, string][],
  current: null as unknown,
  onPrompt: null as ((shown: unknown) => void) | null,
  onRecording: null as ((status: unknown) => void) | null,
  menus: [] as { items: MenuItem[]; popups: unknown[]; closed: boolean }[],
  menuFails: false,
}));

vi.mock("@/ipc/client", () => ({
  onRecordingState: (handler: (status: unknown) => void) => {
    fake.onRecording = handler;
    return () => {};
  },
}));

vi.mock("@/ipc/promptPopup", () => ({
  promptPopupCurrent: () => Promise.resolve(fake.current),
  answerPromptPopup: (id: number, answer: PopupAnswer) => {
    fake.answers.push([id, answer]);
    return Promise.resolve();
  },
  onPromptPopup: (handler: (shown: unknown) => void) => {
    fake.onPrompt = handler;
    return () => {};
  },
}));

// The chevron's native menu: record what it was made with, so a test can
// "click" an item by running its action.
vi.mock("@tauri-apps/api/menu", () => ({
  Menu: {
    new: ({ items }: { items: MenuItem[] }) => {
      if (fake.menuFails) return Promise.reject(new Error("no backend"));
      const menu = { items, popups: [] as unknown[], closed: false };
      fake.menus.push(menu);
      return Promise.resolve({
        popup: (at: unknown) => {
          menu.popups.push(at);
          return Promise.resolve();
        },
        close: () => {
          menu.closed = true;
          return Promise.resolve();
        },
      });
    },
  },
}));

const NINE = new Date(2026, 9, 6, 9, 0).getTime();
const HALF_HOUR = 30 * 60 * 1000;

const ZOOM: DetectionPrompt = {
  signal: { kind: "process", process: "zoom.us" },
  reason: "Zoom is open.",
  updateOnly: false,
  eventId: null,
  canJoin: false,
  test: false,
  title: null,
  startsAtMs: null,
  endsAtMs: null,
  joinService: null,
};

const WITH_LINK: DetectionPrompt = {
  signal: { kind: "calendar", title: "Progress Review", attendees: 3 },
  reason: "“Progress Review” starts in 1 min, with 3 people invited.",
  updateOnly: false,
  eventId: "review-1",
  canJoin: true,
  test: false,
  title: "Progress Review",
  startsAtMs: NINE,
  endsAtMs: NINE + HALF_HOUR,
  joinService: "meet",
};

const NO_LINK: DetectionPrompt = {
  ...WITH_LINK,
  canJoin: false,
  joinService: null,
};

async function showPopup(shown: PopupPrompt) {
  render(<PromptPopup />);
  await act(async () => fake.onPrompt?.(shown));
}

function buttonNames(): string[] {
  return screen.getAllByRole("button").map((button) => button.getAttribute("aria-label") ?? "");
}

/** Open the chevron's menu and return its items. */
async function openMenu(): Promise<MenuItem[]> {
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "More choices" })));
  const menu = fake.menus.at(-1);
  if (menu === undefined) throw new Error("no menu was made");
  return menu.items;
}

beforeEach(() => {
  fake.answers = [];
  fake.current = null;
  fake.menus = [];
  fake.menuFails = false;
});
afterEach(cleanup);

describe("PromptPopup card", () => {
  test("a reminder with a link: title, time range, Join Meet & record", async () => {
    await showPopup({ id: 1, prompt: WITH_LINK });
    expect(screen.getByRole("heading", { name: "Progress Review" })).toBeTruthy();
    expect(screen.getByText(`${formatTime(NINE)} to ${formatTime(NINE + HALF_HOUR)}`)).toBeTruthy();
    expect(buttonNames()).toEqual(["Join Meet and record", "More choices"]);
    const main = screen.getByRole("button", { name: "Join Meet and record" });
    expect(main.textContent).toBe("Join Meet& record");
    await act(async () => fireEvent.click(main));
    expect(fake.answers).toEqual([[1, "joinAndRecord"]]);
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("a reminder without a link: Record is the main button", async () => {
    await showPopup({ id: 2, prompt: NO_LINK });
    expect(screen.getByRole("heading", { name: "Progress Review" })).toBeTruthy();
    expect(buttonNames()).toEqual(["Record", "More choices"]);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Record" })));
    expect(fake.answers).toEqual([[2, "record"]]);
  });

  test("a detection prompt: the reason as the title, Record, and Dismiss in the menu", async () => {
    await showPopup({ id: 3, prompt: ZOOM });
    expect(screen.getByRole("heading", { name: "Zoom is open." })).toBeTruthy();
    expect(screen.getByText("Record this meeting?")).toBeTruthy();
    expect(buttonNames()).toEqual(["Record", "More choices"]);
    const items = await openMenu();
    expect(items.map((item) => item.text)).toEqual(["Dismiss"]);
  });

  test("the card has the accent bar and the rounded popup surface", async () => {
    await showPopup({ id: 4, prompt: WITH_LINK });
    expect(screen.getByTestId("prompt-popup-bar").className).toContain("bg-accent");
    const card = screen.getByRole("region");
    expect(card.className).toContain("rounded-card");
    expect(card.className).toContain("bg-popup");
  });

  test.each([
    ["Join only", "join"],
    ["Record only", "record"],
    ["Open brief", "openBrief"],
    ["Dismiss", "dismiss"],
  ])("the menu's %s sends %s", async (text, answer) => {
    await showPopup({ id: 5, prompt: WITH_LINK });
    const items = await openMenu();
    expect(items.map((item) => item.text)).toEqual([
      "Join only",
      "Record only",
      "Open brief",
      "Dismiss",
    ]);
    const item = items.find((candidate) => candidate.text === text);
    await act(async () => item?.action());
    expect(fake.answers).toEqual([[5, answer]]);
    // Join opens the meeting and leaves the card up; the rest close it.
    expect(screen.queryByRole("heading") !== null).toBe(answer === "join");
  });

  test("the menu pops up under the chevron, and the next one frees the last", async () => {
    await showPopup({ id: 6, prompt: NO_LINK });
    expect((await openMenu()).map((item) => item.text)).toEqual(["Open brief", "Dismiss"]);
    await openMenu();
    expect(fake.menus).toHaveLength(2);
    expect(fake.menus[0]?.closed).toBe(true);
    expect(fake.menus[1]?.popups).toHaveLength(1);
  });

  test("a menu that cannot open says so", async () => {
    fake.menuFails = true;
    await showPopup({ id: 7, prompt: ZOOM });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "More choices" })));
    expect(screen.getByRole("alert").textContent).toContain("Could not open the menu");
  });

  test("a new prompt replaces the old one", async () => {
    await showPopup({ id: 1, prompt: ZOOM });
    await act(async () => fake.onPrompt?.({ id: 2, prompt: NO_LINK }));
    expect(screen.queryByText("Zoom is open.")).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Record" })));
    expect(fake.answers).toEqual([[2, "record"]]);
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

  test("a test reminder says nothing records", async () => {
    await showPopup({ id: 8, prompt: { ...WITH_LINK, eventId: null, test: true } });
    expect(screen.getByText(/test, nothing records/)).toBeTruthy();
  });
});

describe("the card's words", () => {
  test("the main button names the service", () => {
    expect(joinWords("meet")).toBe("Join Meet");
    expect(joinWords("zoom")).toBe("Join Zoom");
    expect(joinWords("teams")).toBe("Join Teams");
    expect(joinWords("other")).toBe("Join");
    expect(joinWords(null)).toBe("Join");
  });

  test("the menu offers Join and Record only with a link, Open brief only for a reminder", () => {
    const texts = (prompt: DetectionPrompt) => choicesFor(prompt).map((choice) => choice.answer);
    expect(texts(WITH_LINK)).toEqual(["join", "record", "openBrief", "dismiss"]);
    expect(texts(NO_LINK)).toEqual(["openBrief", "dismiss"]);
    expect(texts(ZOOM)).toEqual(["dismiss"]);
  });
});
