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
 *   on what stops working and the **Open System Settings** button, like the
 *   audio permission screen;
 * - any other read error, through the shared error screen.
 *
 * The list re-reads every `calendar.refresh_minutes` and whenever the window
 * comes to the front, so a meeting added in Calendar.app shows up without a
 * restart.
 *
 * Solo blocks (fewer than `detection.min_attendees` people) are greyed, not
 * hidden, so the day still reads true. Nothing here acts on them; nothing here
 * acts on any event yet — the click to open a brief is TUR-32's.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  openCalendarSettings,
  type TodayEvent,
  type TodaysMeetings,
  todaysMeetings,
} from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Button, ButtonRow, cardVariants, RowLabel, RowValue, rowVariants } from "./primitives";
import { Checking, ErrorState } from "./states";

/** How often to re-read before the first answer says otherwise. */
const DEFAULT_REFRESH_MINUTES = 15;

export const CALENDAR_DENIED_COPY =
  "meet-ai can't read your calendar, so Today and meeting reminders are off.";

type State =
  | { kind: "loading" }
  | { kind: "ready"; today: TodaysMeetings }
  | { kind: "denied" }
  | { kind: "error"; error: UiError };

export function TodayPane() {
  const [state, setState] = useState<State>({ kind: "loading" });
  const [refreshMinutes, setRefreshMinutes] = useState(DEFAULT_REFRESH_MINUTES);
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
      setState(error.kind === "calendar-denied" ? { kind: "denied" } : { kind: "error", error });
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
        <div className="state" role="alert">
          <p className="state__body">{CALENDAR_DENIED_COPY}</p>
          <ButtonRow>
            <Button tone="primary" onClick={() => void openSettings()}>
              Open System Settings
            </Button>
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
  return (
    <li
      className={cn(rowVariants({ divided: false }), event.solo && "opacity-50")}
      aria-disabled={event.solo || undefined}
      title={
        event.solo
          ? `Fewer than ${minAttendees} people, so meet-ai treats it as a solo block`
          : undefined
      }
    >
      <RowLabel
        name={event.title}
        detail={`${formatTime(event.startMs)} – ${formatTime(event.endMs)}`}
        mono={false}
      />
      <RowValue>{formatAttendees(event.attendees)}</RowValue>
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

async function openSettings() {
  try {
    await openCalendarSettings();
  } catch {
    // The deep link and its fallback both failed. The sentence above still
    // says what to change; an error screen on top of it would add nothing.
  }
}
