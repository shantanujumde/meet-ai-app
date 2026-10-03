import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { TodayEvent, TodaysMeetings } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { CALENDAR_DENIED_COPY, formatAttendees, formatTime, TodayPane } from "./TodayPane";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { todaysMeetings, openCalendarSettings } = ipc;

const NINE = Date.UTC(2026, 9, 3, 9, 0);
const HALF_HOUR = 30 * 60_000;

function event(over: Partial<TodayEvent> = {}): TodayEvent {
  return {
    id: "standup",
    title: "Standup",
    startMs: NINE,
    endMs: NINE + HALF_HOUR,
    attendees: 4,
    solo: false,
    ...over,
  };
}

function day(events: TodayEvent[], over: Partial<TodaysMeetings> = {}): TodaysMeetings {
  return { events, refreshMinutes: 15, minAttendees: 2, ...over };
}

const DENIED: UiError = {
  domain: "app",
  kind: "calendar-denied",
  message: "meet-ai does not have permission to read your calendar",
};

/** Render, then let the first read land. */
async function show() {
  render(<TodayPane />);
  await act(async () => {});
}

describe("TodayPane", () => {
  afterEach(() => vi.useRealTimers());

  test("says it is reading until the calendar answers", async () => {
    let answer: (today: TodaysMeetings) => void = () => {};
    todaysMeetings.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    render(<TodayPane />);

    expect(screen.getByRole("status")).toHaveTextContent("Reading your calendar…");

    await act(async () => answer(day([event()])));
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByText("Standup")).toBeTruthy();
  });

  test("an empty day says so", async () => {
    todaysMeetings.mockResolvedValue(day([]));
    await show();

    expect(screen.getByText("No meetings on your calendar today.")).toBeTruthy();
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("denied access is its own screen, not an empty day", async () => {
    todaysMeetings.mockRejectedValue(DENIED);
    await show();

    expect(screen.getByRole("alert")).toHaveTextContent(CALENDAR_DENIED_COPY);
    expect(screen.queryByText("No meetings on your calendar today.")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }));
    expect(openCalendarSettings).toHaveBeenCalledTimes(1);
  });

  test("lists time, title and attendee count; solo blocks are greyed", async () => {
    todaysMeetings.mockResolvedValue(
      day([
        event(),
        event({ id: "focus", title: "Focus time", attendees: 0, solo: true }),
        event({ id: "1on1", title: "1:1", attendees: 1, solo: true }),
      ]),
    );
    await show();

    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("Standup");
    expect(rows[0]).toHaveTextContent(`${formatTime(NINE)} – ${formatTime(NINE + HALF_HOUR)}`);
    expect(rows[0]).toHaveTextContent("4 people");
    expect(rows[0]).not.toHaveAttribute("aria-disabled");

    expect(rows[1]).toHaveTextContent("Just you");
    expect(rows[1]).toHaveAttribute("aria-disabled", "true");
    expect(rows[1]).toHaveClass("opacity-50");
    expect(rows[2]).toHaveTextContent("1 person");
    expect(rows[2]).toHaveAttribute("aria-disabled", "true");

    // Nothing in the list is clickable yet (TUR-32 adds the brief).
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("another read error shows the error screen", async () => {
    todaysMeetings.mockRejectedValue({
      domain: "app",
      kind: "calendar-unreachable",
      message: "could not reach ICS: timed out",
    } satisfies UiError);
    await show();

    expect(screen.getByRole("alert")).toHaveTextContent("could not reach ICS: timed out");
  });

  test("re-reads every refresh_minutes", async () => {
    vi.useFakeTimers();
    todaysMeetings.mockResolvedValue(day([], { refreshMinutes: 5 }));
    await show();
    expect(todaysMeetings).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5 * 60_000);
    });
    expect(todaysMeetings).toHaveBeenCalledTimes(2);
  });

  test("re-reads when the window comes to the front", async () => {
    todaysMeetings.mockResolvedValue(day([]));
    await show();

    todaysMeetings.mockRejectedValue(DENIED);
    await act(async () => {
      fireEvent.focus(window);
    });
    expect(todaysMeetings).toHaveBeenCalledTimes(2);
    // Access revoked while the app was open: the denied screen replaces the list.
    expect(screen.getByRole("alert")).toHaveTextContent(CALENDAR_DENIED_COPY);
  });

  test("attendee counts read as words", () => {
    expect([0, 1, 7].map(formatAttendees)).toEqual(["Just you", "1 person", "7 people"]);
  });
});
