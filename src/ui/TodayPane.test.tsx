import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { TodayEvent, TodaysMeetings } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { EMPTY_DAY, SIGN_IN_TO_SEE_TODAY } from "./calendar/copy";
import { CALENDAR_DENIED_COPY, formatAttendees, formatTime, TodayPane } from "./TodayPane";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { todaysMeetings, openPrivacySettings, calendarRefreshMinutes } = ipc;

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

/** Where a click landed: the path and query the router is now at. */
function Location() {
  const location = useLocation();
  return <p>at {`${location.pathname}${location.search}`}</p>;
}

/** The pane inside a router (its rows link to the brief, TUR-32). */
function renderPane() {
  return render(
    <MemoryRouter>
      <Routes>
        <Route path="/" element={<TodayPane />} />
        <Route path="/brief" element={<Location />} />
      </Routes>
    </MemoryRouter>,
  );
}

/** Render, then let the first read land. */
async function show() {
  renderPane();
  await act(async () => {});
}

describe("TodayPane", () => {
  afterEach(() => vi.useRealTimers());

  test("says it is reading until the calendar answers", async () => {
    let answer: (today: TodaysMeetings) => void = () => {};
    todaysMeetings.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    renderPane();

    expect(screen.getByRole("status")).toHaveTextContent("Reading your calendar…");

    await act(async () => answer(day([event()])));
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByText("Standup")).toBeTruthy();
  });

  test("an empty day says so", async () => {
    todaysMeetings.mockResolvedValue(day([]));
    await show();

    expect(screen.getByText(EMPTY_DAY)).toBeTruthy();
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  test("denied access is its own screen, not an empty day", async () => {
    todaysMeetings.mockRejectedValue(DENIED);
    await show();

    expect(screen.getByRole("alert")).toHaveTextContent(CALENDAR_DENIED_COPY);
    expect(screen.queryByText(EMPTY_DAY)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }));
    expect(openPrivacySettings).toHaveBeenCalledWith("calendars");
  });

  test("Check again re-reads, and a granted calendar replaces the denied screen", async () => {
    todaysMeetings.mockRejectedValue(DENIED);
    await show();

    todaysMeetings.mockResolvedValue(day([event()]));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    });
    expect(todaysMeetings).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("Standup")).toBeTruthy();
  });

  test("the denied screen needs the app domain, not just the kind", async () => {
    todaysMeetings.mockRejectedValue({ ...DENIED, domain: "stt" } satisfies UiError);
    await show();

    expect(screen.queryByText(CALENDAR_DENIED_COPY)).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(DENIED.message);
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
    expect(rows[0]).toHaveTextContent(`${formatTime(NINE)} to ${formatTime(NINE + HALF_HOUR)}`);
    expect(rows[0]).toHaveTextContent("4 people");
    expect(rows[0]).not.toHaveAttribute("aria-disabled");

    expect(rows[1]).toHaveTextContent("Just you");
    expect(rows[1]).toHaveAttribute("aria-disabled", "true");
    expect(rows[1]).toHaveClass("opacity-50");
    expect(rows[2]).toHaveTextContent("1 person");
    expect(rows[2]).toHaveAttribute("aria-disabled", "true");

    // Only the real meeting links to a brief; solo blocks are not clickable.
    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(rows[1]?.querySelector("a")).toBeFalsy();
    expect(rows[2]?.querySelector("a")).toBeFalsy();
  });

  test("clicking a meeting opens its pre-meeting brief (TUR-32)", async () => {
    todaysMeetings.mockResolvedValue(day([event({ title: "R&D sync" })]));
    await show();

    fireEvent.click(screen.getByRole("link", { name: /R&D sync/ }));
    expect(screen.getByText("at /brief?title=R%26D+sync")).toBeTruthy();
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

  test("a failed read still re-reads on the configured interval", async () => {
    vi.useFakeTimers();
    todaysMeetings.mockRejectedValue(DENIED);
    calendarRefreshMinutes.mockResolvedValue(3);
    await show();
    expect(calendarRefreshMinutes).toHaveBeenCalledTimes(1);
    expect(todaysMeetings).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3 * 60_000);
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

// TUR-49: Windows and Linux have no Calendar app, so with no sign-in there is
// nothing to read, and the pane says how to fix that instead of "no meetings".
describe("TodayPane with no calendar to read", () => {
  const OFF_MAC = {
    calendarAppAvailable: false,
    calendarApp: false,
    configured: ["google" as const, "microsoft" as const],
    connected: [],
  };

  test("off macOS with no sign-in, it asks to sign in with either provider", async () => {
    ipc.calendarSources.mockResolvedValue(OFF_MAC);
    await show();

    expect(screen.getByText(SIGN_IN_TO_SEE_TODAY)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeTruthy();
    expect(screen.queryByText(EMPTY_DAY)).toBeNull();
    expect(todaysMeetings).not.toHaveBeenCalled();
  });

  test("signing in from the empty state reads the calendar", async () => {
    ipc.calendarSources.mockResolvedValue(OFF_MAC);
    await show();

    ipc.calendarSources.mockResolvedValue({ ...OFF_MAC, connected: ["microsoft"] });
    todaysMeetings.mockResolvedValue(day([event()]));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign in with Microsoft" }));
    });

    expect(ipc.calendarConnect).toHaveBeenCalledWith("microsoft");
    expect(screen.getByText("Standup")).toBeTruthy();
    expect(screen.queryByText(SIGN_IN_TO_SEE_TODAY)).toBeNull();
  });

  test("a missing client id names the config key", async () => {
    ipc.calendarSources.mockResolvedValue(OFF_MAC);
    ipc.calendarConnect.mockRejectedValue({
      domain: "app",
      kind: "calendar-not-configured",
      message: "Google sign-in is not set up",
    });
    await show();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Add calendar.google.client_id to config.jsonc (see SETUP.md)",
    );
  });

  test("off macOS with a sign-in, the day is read as usual", async () => {
    ipc.calendarSources.mockResolvedValue({ ...OFF_MAC, connected: ["google"] });
    todaysMeetings.mockResolvedValue(day([]));
    await show();
    expect(screen.getByText(EMPTY_DAY)).toBeTruthy();
    expect(screen.queryByText(SIGN_IN_TO_SEE_TODAY)).toBeNull();
  });
});
