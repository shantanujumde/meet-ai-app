import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { DetectionPrompt } from "@/ipc/client";
import type { PopupAnswer, PopupPrompt } from "@/ipc/promptPopup";
import type { RecordingStatus } from "@/ipc/types";
import { choicesFor, joinWords, layoutOf, PromptPopup, startChoicesFor } from "./PromptPopup";
import { FADE_MS, secondsLeft } from "./promptCards";
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

// The native menus: record what each was made with, so a test can "click"
// an item by running its action.
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
/** A close time no test reaches without moving the clock. */
const LATER = Date.now() + 60 * 60 * 1000;

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
  headline: "Zoom call",
  app: "Zoom",
};

const AUDIO: DetectionPrompt = {
  ...ZOOM,
  signal: { kind: "audio_activity" },
  reason: "Your microphone and speakers are both in use, like on a call.",
  headline: "Audio activity",
  app: null,
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
  headline: "Progress Review",
  app: null,
};

const NO_LINK: DetectionPrompt = {
  ...WITH_LINK,
  canJoin: false,
  joinService: null,
};

/** The prompt card for `prompt`, as Rust sends it. */
function ask(id: number, prompt: DetectionPrompt, closesAtMs = LATER): PopupPrompt {
  return { id, card: { kind: "prompt", prompt }, closesAtMs };
}

/** The countdown card, as TUR-144 shows it. */
function ended(id: number, closesAtMs: number, seconds = 10): PopupPrompt {
  return { id, card: { kind: "countdown", line: "Zoom call ended", seconds }, closesAtMs };
}

async function showPopup(shown: PopupPrompt) {
  render(<PromptPopup />);
  await act(async () => fake.onPrompt?.(shown));
}

function buttonNames(): string[] {
  return screen
    .getAllByRole("button")
    .map((button) => button.getAttribute("aria-label") ?? button.textContent ?? "");
}

/** Open the card's menu (`name` is its button) and return its items. */
async function openMenu(name = "More choices"): Promise<MenuItem[]> {
  await act(async () => fireEvent.click(screen.getByRole("button", { name })));
  const menu = fake.menus.at(-1);
  if (menu === undefined) throw new Error("no menu was made");
  return menu.items;
}

async function press(name: string) {
  await act(async () => fireEvent.click(screen.getByRole("button", { name })));
}

async function setPhase(phase: RecordingStatus["phase"]) {
  const status: RecordingStatus = { phase, meetingId: "m", startedAtMs: 1, error: null };
  await act(async () => fake.onRecording?.(status));
}

/** Turn motion on or off, as `prefers-reduced-motion` would. */
function reduceMotion(reduce: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) => ({ matches: reduce && query.includes("reduce"), media: query })),
  );
}

beforeEach(() => {
  fake.answers = [];
  fake.current = null;
  fake.menus = [];
  fake.menuFails = false;
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the reminder card (TUR-108)", () => {
  test("a reminder with a link: title, time range, Join Meet & record", async () => {
    await showPopup(ask(1, WITH_LINK));
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
    await showPopup(ask(2, NO_LINK));
    expect(screen.getByRole("heading", { name: "Progress Review" })).toBeTruthy();
    expect(buttonNames()).toEqual(["Record", "More choices"]);
    await press("Record");
    expect(fake.answers).toEqual([[2, "record"]]);
  });

  test("the card has the accent bar and the rounded popup surface", async () => {
    await showPopup(ask(4, WITH_LINK));
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
    await showPopup(ask(5, WITH_LINK));
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
    await showPopup(ask(6, NO_LINK));
    expect((await openMenu()).map((item) => item.text)).toEqual(["Open brief", "Dismiss"]);
    await openMenu();
    expect(fake.menus).toHaveLength(2);
    expect(fake.menus[0]?.closed).toBe(true);
    expect(fake.menus[1]?.popups).toHaveLength(1);
  });

  test("a menu that cannot open says so", async () => {
    fake.menuFails = true;
    await showPopup(ask(7, NO_LINK));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "More choices" })));
    expect(screen.getByRole("alert").textContent).toContain("Could not open the menu");
  });

  test("a test reminder says nothing records", async () => {
    await showPopup(ask(8, { ...WITH_LINK, eventId: null, test: true }));
    expect(screen.getByText(/test, nothing records/)).toBeTruthy();
  });
});

describe("the detection card (TUR-147)", () => {
  test("names the app in one line, with our icon, Record, Not now and ⋯", async () => {
    await showPopup(ask(3, ZOOM));
    expect(screen.getByRole("heading", { name: "Zoom call" })).toBeTruthy();
    expect(screen.queryByText("Zoom is open.")).toBeNull();
    expect(buttonNames()).toEqual(["More choices", "Record Zoom call", "Not now, do not record"]);
    expect(screen.getByRole("button", { name: "Record Zoom call" }).textContent).toBe("Record");
    expect(screen.getByRole("button", { name: "Not now, do not record" }).textContent).toBe(
      "Not now",
    );
    const icon = screen.getByRole("region").querySelector("img");
    expect(icon?.getAttribute("src")).toContain("meet-ai-appicon");
    expect(icon?.getAttribute("alt")).toBe("");
  });

  test("Record records", async () => {
    await showPopup(ask(3, ZOOM));
    await press("Record Zoom call");
    expect(fake.answers).toEqual([[3, "record"]]);
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("Not now dismisses", async () => {
    await showPopup(ask(3, ZOOM));
    await press("Not now, do not record");
    expect(fake.answers).toEqual([[3, "dismiss"]]);
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("the ⋯ menu holds Never for Zoom, which answers neverFor", async () => {
    await showPopup(ask(9, ZOOM));
    const more = screen.getByRole("button", { name: "More choices" });
    expect(more.getAttribute("aria-haspopup")).toBe("menu");
    const items = await openMenu();
    expect(items.map((item) => item.text)).toEqual(["Never for Zoom"]);
    await act(async () => items[0]?.action());
    expect(fake.answers).toEqual([[9, "neverFor"]]);
    expect(screen.queryByRole("region")).toBeNull();
    expect(fake.menus[0]?.popups).toHaveLength(1);
  });

  test("audio activity has no app to never ask about, so no ⋯", async () => {
    await showPopup(ask(10, AUDIO));
    expect(screen.getByRole("heading", { name: "Audio activity" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "More choices" })).toBeNull();
    expect(buttonNames()).toEqual(["Record Audio activity", "Not now, do not record"]);
  });

  test("a narrow rounded card with a soft shadow, none on macOS where the OS draws it", async () => {
    await showPopup(ask(3, ZOOM));
    const card = screen.getByRole("region");
    for (const name of ["rounded-card", "bg-popup", "border-rim", "shadow-floating"]) {
      expect(card.className).toContain(name);
    }
    expect(card.className).toContain("[[data-os=macos]_&]:shadow-none");
    // The window's room for that shadow, gone on macOS too.
    expect(card.parentElement?.className).toContain("p-4");
    expect(card.parentElement?.className).toContain("[[data-os=macos]_&]:p-0");
    // The "⋯" is the small 24px control, not the default 32px icon button.
    const more = screen.getByRole("button", { name: "More choices" }).className;
    expect(more).toContain("size-(--control-h-regular)");
    expect(more).not.toContain("size-(--control-h-large)");
  });

  test("slides in, and only when motion is welcome", async () => {
    await showPopup(ask(3, ZOOM));
    const card = screen.getByRole("region");
    expect(card.className).toContain("motion-safe:starting:translate-x-4");
    expect(card.className).toContain("motion-safe:starting:opacity-0");
    expect(card.className).toContain("motion-reduce:transition-none");
  });

  test("a menu that cannot open says so in the card's line", async () => {
    fake.menuFails = true;
    await showPopup(ask(3, ZOOM));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "More choices" })));
    expect(screen.getByRole("alert").textContent).toContain("Could not open the menu");
  });

  test("Escape is Not now", async () => {
    await showPopup(ask(3, ZOOM));
    await act(async () => fireEvent.keyDown(window, { key: "Escape" }));
    expect(fake.answers).toEqual([[3, "dismiss"]]);
  });
});

describe("the countdown card (TUR-147)", () => {
  test("names the call, counts down from 10, and offers Stop now and Keep recording", async () => {
    vi.useFakeTimers({ now: NINE });
    await showPopup(ended(11, NINE + 10_000));
    expect(screen.getByRole("heading", { name: "Zoom call ended" })).toBeTruthy();
    expect(screen.getByRole("timer").getAttribute("aria-label")).toBe("Stopping in 10 seconds");
    expect(buttonNames()).toEqual(["Stop now and save the recording", "Keep recording"]);
    const ring = screen.getByTestId("countdown-ring");
    const full = Number(ring.getAttribute("stroke-dashoffset"));
    expect(full).toBe(0);
    await act(async () => vi.advanceTimersByTime(3_000));
    expect(screen.getByRole("timer").getAttribute("aria-label")).toBe("Stopping in 7 seconds");
    expect(Number(ring.getAttribute("stroke-dashoffset"))).toBeGreaterThan(full);
    await act(async () => vi.advanceTimersByTime(6_000));
    expect(screen.getByRole("timer").getAttribute("aria-label")).toBe("Stopping in 1 second");
    expect(fake.answers).toEqual([]);
  });

  test("a window that loads late counts from where the countdown is", async () => {
    vi.useFakeTimers({ now: NINE + 4_000 });
    fake.current = ended(12, NINE + 10_000);
    await act(async () => {
      render(<PromptPopup />);
    });
    expect(screen.getByRole("timer").getAttribute("aria-label")).toBe("Stopping in 6 seconds");
  });

  test.each([
    ["Stop now and save the recording", "stopNow"],
    ["Keep recording", "keepRecording"],
  ])("%s sends %s and closes the card", async (name, answer) => {
    await showPopup(ended(13, LATER));
    await press(name);
    expect(fake.answers).toEqual([[13, answer]]);
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("the recording going on keeps it up; the recording stopping closes it", async () => {
    await showPopup(ended(14, LATER));
    await setPhase("recording");
    expect(screen.getByRole("timer")).toBeTruthy();
    expect(fake.answers).toEqual([]);
    await setPhase("idle");
    expect(screen.queryByRole("timer")).toBeNull();
    expect(fake.answers).toEqual([[14, "dismiss"]]);
  });

  test("Escape is Keep recording", async () => {
    await showPopup(ended(15, LATER));
    await act(async () => fireEvent.keyDown(window, { key: "Escape" }));
    expect(fake.answers).toEqual([[15, "keepRecording"]]);
  });

  test("the seconds left never go below 0 or above the countdown", () => {
    expect(secondsLeft(10_000, 10, 0)).toBe(10);
    expect(secondsLeft(10_000, 10, 100)).toBe(10);
    expect(secondsLeft(10_000, 10, 1_000)).toBe(9);
    expect(secondsLeft(10_000, 10, 9_999)).toBe(1);
    expect(secondsLeft(10_000, 10, 12_000)).toBe(0);
    expect(secondsLeft(60_000, 10, 0)).toBe(10);
  });
});

describe("every card", () => {
  test("a new card replaces the old one", async () => {
    await showPopup(ask(1, ZOOM));
    await act(async () => fake.onPrompt?.(ask(2, NO_LINK)));
    expect(screen.queryByText("Zoom call")).toBeNull();
    await press("Record");
    expect(fake.answers).toEqual([[2, "record"]]);
  });

  test("a window that loads late shows the card already up", async () => {
    fake.current = ask(7, ZOOM);
    await act(async () => {
      render(<PromptPopup />);
    });
    expect(screen.getByText("Zoom call")).toBeTruthy();
  });

  test("a recording started elsewhere closes a prompt without recording", async () => {
    await showPopup(ask(5, ZOOM));
    await setPhase("recording");
    expect(screen.queryByRole("button")).toBeNull();
    expect(fake.answers).toEqual([[5, "dismiss"]]);
  });

  test("with motion, the card fades out when its time is up, then goes", async () => {
    reduceMotion(false);
    vi.useFakeTimers({ now: NINE });
    await showPopup(ask(3, ZOOM, NINE + 20_000));
    const card = screen.getByRole("region");
    expect(card.dataset.leaving).toBeUndefined();
    await act(async () => vi.advanceTimersByTime(20_000));
    expect(card.dataset.leaving).toBe("");
    expect(card.className).toContain("opacity-0");
    await act(async () => vi.advanceTimersByTime(FADE_MS));
    expect(screen.queryByRole("region")).toBeNull();
    expect(fake.answers).toEqual([]);
  });

  test("with motion, an answer fades the card out too", async () => {
    reduceMotion(false);
    vi.useFakeTimers({ now: NINE });
    await showPopup(ask(3, ZOOM));
    await press("Record Zoom call");
    expect(fake.answers).toEqual([[3, "record"]]);
    expect(screen.getByRole("region").dataset.leaving).toBe("");
    await act(async () => vi.advanceTimersByTime(FADE_MS));
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("with Reduce Motion, it goes at once", async () => {
    reduceMotion(true);
    vi.useFakeTimers({ now: NINE });
    await showPopup(ended(16, NINE + 10_000));
    await act(async () => vi.advanceTimersByTime(10_000));
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("a new card while one fades out shows in full", async () => {
    reduceMotion(false);
    await showPopup(ask(3, ZOOM));
    await press("Not now, do not record");
    await act(async () => fake.onPrompt?.(ended(4, LATER)));
    expect(screen.getByRole("region").dataset.leaving).toBeUndefined();
    expect(screen.getByRole("heading", { name: "Zoom call ended" })).toBeTruthy();
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

  test("the ⋯ menu names the app, and is empty without one", () => {
    expect(startChoicesFor(ZOOM)).toEqual([{ answer: "neverFor", text: "Never for Zoom" }]);
    expect(startChoicesFor(AUDIO)).toEqual([]);
  });

  test("each card picks its layout as Rust sizes its window", () => {
    expect(layoutOf(ask(1, WITH_LINK).card)).toBe("reminder");
    expect(layoutOf(ask(1, ZOOM).card)).toBe("start");
    expect(layoutOf(ask(1, AUDIO).card)).toBe("start");
    expect(layoutOf(ended(1, LATER).card)).toBe("countdown");
  });
});
