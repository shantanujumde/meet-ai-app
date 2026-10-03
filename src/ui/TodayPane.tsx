/**
 * "Today": the day's meetings, read straight from Calendar.app (TUR-28,
 * `docs/problem.md` item 38). No login and no setup — macOS asks once for
 * calendar access, and that is all.
 *
 * Four states, each with its own screen:
 *
 * - loading, only before the first answer (a refresh keeps the list up);
 * - no meetings today, when the calendars were read and hold nothing;
 * - calendar access denied, which is never shown as an empty day: one line
 *   on what stops working, **Open System Settings** and **Check again**, like
 *   the audio permission screen;
 * - any other read error, through the shared error screen.
 *
 * The list re-reads every `calendar.refresh_minutes` and whenever the window
 * comes to the front, so a meeting added in Calendar.app shows up without a
 * restart. The interval follows the config even while reads fail.
 *
 * Solo blocks (fewer than `detection.min_attendees` people) are greyed, not
 * hidden, so the day still reads true, and nothing here acts on them. Every
 * other event is a link to its pre-meeting brief (TUR-32).
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router";
import {
  calendarRefreshMinutes,
  type TodayEvent,
  type TodaysMeetings,
  todaysMeetings,
} from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { NO_MEETINGS_TODAY } from "@/lib/constants";
import { briefPath } from "@/lib/routes";
import { openSettings } from "./PrivacyButtons";
import { Button, ButtonRow, cardVariants, RowLabel, RowValue, rowVariants } from "./primitives";
import { Checking, ErrorState } from "./states";

export const CALENDAR_DENIED_COPY =
  "meet-ai can't read your calendar, so Today and meeting reminders are off.";

type State =
  | { kind: "loading" }
  | { kind: "ready"; today: TodaysMeetings }
  | { kind: "denied" }
  | { kind: "error"; error: UiError };

export function TodayPane() {
  const [state, setState] = useState<State>({ kind: "loading" });
  // The default until Rust answers; every answer, or the cheap config read
  // after a failed one, replaces it.
  const [refreshMinutes, setRefreshMinutes] = useState(NO_MEETINGS_TODAY.refreshMinutes);
  // Only the latest read may land: a slow read that started before a newer
  // one must not overwrite it.
  const latest = useRef(0);

  const load = useCallback(async () => {
    const id = ++latest.current;
    try {
      const today = await todaysMeetings();
      if (id !== latest.current) return;
      setState({ kind: "ready", today });
      setRefreshMinutes(today.refreshMinutes);
    } catch (thrown) {
      if (id !== latest.current) return;
      const error = toUiError(thrown);
      setState(isDenied(error) ? { kind: "denied" } : { kind: "error", error });
      // A failed read carries no settings, so ask for the interval alone: a
      // denied calendar is still re-read on the configured schedule.
      try {
        const minutes = await calendarRefreshMinutes();
        if (id === latest.current) setRefreshMinutes(minutes);
      } catch {
        // Keep the current interval; the next read tries again.
      }
    }
  }, []);

  useEffect(() => {
    void load();
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      // Unmounted: whatever is still in flight has nowhere to land.
      latest.current++;
    };
  }, [load]);

  useEffect(() => {
    // A zero or negative setting would spin; one minute is the floor.
    const every = Math.max(1, refreshMinutes) * 60_000;
    const timer = window.setInterval(() => void load(), every);
    return () => window.clearInterval(timer);
  }, [load, refreshMinutes]);

  return (
    <section className="section" aria-labelledby="today-heading">
      <h2 className="section__title" id="today-heading">
        Today
      </h2>
      <TodayBody state={state} onRetry={() => void load()} />
    </section>
  );
}

function TodayBody({ state, onRetry }: { state: State; onRetry: () => void }) {
  switch (state.kind) {
    case "loading":
      return <Checking label="Reading your calendar…" />;
    case "denied":
      return (
        <div className="state state--error" role="alert">
          <p className="state__body">{CALENDAR_DENIED_COPY}</p>
          <ButtonRow>
            <Button tone="primary" onClick={() => void openSettings("calendars")}>
              Open System Settings
            </Button>
            <Button onClick={onRetry}>Check again</Button>
          </ButtonRow>
        </div>
      );
    case "error":
      return <ErrorState error={state.error} onRemedy={onRetry} />;
    case "ready":
      if (state.today.events.length === 0) {
        return <p className="state__body">No meetings on your calendar today.</p>;
      }
      return (
        <ul className={cardVariants({ flush: true })} aria-label="Today's meetings">
          {state.today.events.map((event) => (
            <TodayRow key={event.id} event={event} minAttendees={state.today.minAttendees} />
          ))}
        </ul>
      );
  }
}

function TodayRow({ event, minAttendees }: { event: TodayEvent; minAttendees: number }) {
  const body = (
    <>
      <RowLabel
        name={event.title}
        detail={`${formatTime(event.startMs)} – ${formatTime(event.endMs)}`}
        mono={false}
      />
      <RowValue>{formatAttendees(event.attendees)}</RowValue>
    </>
  );
  if (event.solo) {
    return (
      <li
        className={cn(rowVariants({ divided: false }), "opacity-50")}
        aria-disabled
        title={`Fewer than ${minAttendees} people, so meet-ai treats it as a solo block`}
      >
        {body}
      </li>
    );
  }
  // TUR-32: a real meeting opens its pre-meeting brief. A link, so Tab and
  // Enter reach it like any other.
  return (
    <li>
      <Link
        to={briefPath(event.title)}
        className={cn(rowVariants({ divided: false }), "hover:bg-row-hover")}
      >
        {body}
      </Link>
    </li>
  );
}

const TIME = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

/** A wall-clock time in the user's locale, like "9:30 AM". */
export function formatTime(ms: number): string {
  return TIME.format(new Date(ms));
}

/** "Just you", "1 person", "4 people". */
export function formatAttendees(count: number): string {
  if (count === 0) return "Just you";
  return count === 1 ? "1 person" : `${count} people`;
}

/** Calendar access is off: the one error with its own screen. */
function isDenied(error: UiError): boolean {
  return error.domain === "app" && error.kind === "calendar-denied";
}
