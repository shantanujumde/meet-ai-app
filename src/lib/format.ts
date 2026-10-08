/**
 * Formatting helpers shared by the screens.
 *
 * Pure functions with no React in them, so they are testable on their own —
 * which matters, because "190 MB" versus "181 MiB" and "Today" versus a raw
 * date are exactly the details that make an interface feel hand-made or not.
 */

import type { RecordingState } from "@/ipc/types";
import { currentOs, type Os, osText } from "@/lib/osText";

/**
 * Bytes as a person would say them.
 *
 * Decimal units (MB = 10^6), which is what macOS itself shows and what the
 * model's own download page says. Binary units would render the pinned
 * 190,098,681-byte model as "181 MB" and make the number look wrong next to
 * every other source.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "unknown size";
  if (bytes < 1_000) return `${bytes} bytes`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1_000)} KB`;
  if (bytes < 1_000_000_000) return `${Math.round(bytes / 1_000_000)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/** `HH:MM:SS` from a millisecond duration. Used for the recording timer. */
export function formatElapsed(milliseconds: number): string {
  const total = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(hours)}:${pad(minutes)}:${pad(seconds)}`;
}

/**
 * A meeting's length as the header's meta line says it: "< 1 min", "50 min",
 * "1 h 5 min", "2 h". Rounded to the minute — the exact second is the
 * transcript's job, not the header's.
 */
export function formatDuration(milliseconds: number): string {
  const minutes = Math.round(Math.max(0, milliseconds) / 60_000);
  if (minutes === 0) return "< 1 min";
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

/** A transcript timestamp (`HH:MM:SS`) as milliseconds, or `null` if it is not one. */
export function timestampToMs(timestamp: string | null): number | null {
  if (!timestamp) return null;
  const match = /^(\d+):(\d{2}):(\d{2})$/.exec(timestamp);
  if (!match) return null;
  const [, hours, minutes, seconds] = match;
  return ((Number(hours) * 60 + Number(minutes)) * 60 + Number(seconds)) * 1000;
}

/**
 * A meeting's date, as short as it can be while staying unambiguous.
 *
 * `today` is a parameter rather than a call to `new Date()` so the result is
 * testable and so a window left open overnight can be re-rendered against the
 * new day rather than insisting yesterday is still "Today".
 */
export function formatRelativeDate(date: string | null, today: Date = new Date()): string {
  if (!date) return "No date";
  const parsed = parseLocalDate(date);
  if (!parsed) return date;

  const days = wholeDaysBetween(parsed, today);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days > 1 && days < 7) {
    return parsed.toLocaleDateString(undefined, { weekday: "long" });
  }
  const sameYear = parsed.getFullYear() === today.getFullYear();
  return parsed.toLocaleDateString(
    undefined,
    sameYear
      ? { day: "numeric", month: "short" }
      : { day: "numeric", month: "short", year: "numeric" },
  );
}

/**
 * Parse `YYYY-MM-DD` as a *local* date.
 *
 * `new Date("2026-09-01")` parses as UTC midnight, which in a timezone west of
 * Greenwich is the previous day — so a meeting recorded this morning would show
 * as "Yesterday". Building the date from its parts avoids that entirely.
 */
function parseLocalDate(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const [, year, month, day] = match;
  const parsed = new Date(Number(year), Number(month) - 1, Number(day));
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

/** Calendar days between two dates, ignoring the time of day. */
function wholeDaysBetween(earlier: Date, later: Date): number {
  const startOf = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const millisecondsPerDay = 24 * 60 * 60 * 1000;
  return Math.round((startOf(later).getTime() - startOf(earlier).getTime()) / millisecondsPerDay);
}

/**
 * What an interrupted meeting is called in the list (TUR-97, SPEC §3.1).
 *
 * One word, and a calm one: the audio and transcript up to the cut are good,
 * so this is neither "Failed" nor "Corrupt" — but it is not a finished
 * meeting either, and must not look like one.
 */
export const INTERRUPTED_LABEL = "Interrupted";

/**
 * What a meeting that a backup stop ended says next to its length (TUR-145):
 * the computer went to sleep, or no one spoke for 10 minutes. `null` for a
 * meeting that ended any other way.
 */
export function backupStopLabel(state: RecordingState, os: Os = currentOs()): string | null {
  if (state === "stopped-for-sleep")
    return `Stopped when ${osText("thisComputer", os)} went to sleep`;
  if (state === "stopped-for-silence") return "Stopped after 10 minutes of silence";
  return null;
}

/**
 * How the meeting list marks a meeting whose notes were switched off
 * (TUR-12, SPEC A11): its transcript is never sent to an agent. Words, not a
 * colour, so it reads the same to everyone.
 */
export const NOTES_OFF_LABEL = "Notes off";

/**
 * The one-line explanation an interrupted meeting shows when it is opened.
 *
 * `audioMs` is the header-declared length (SPEC A5), so the time quoted is
 * exactly as far as a player will go — never the frame-count sum, which after
 * a crash runs up to one checkpoint past the audio that exists.
 */
export function describeInterruption(audioMs: number | null): string {
  if (audioMs === null || audioMs < 1000) {
    return "Recording stopped unexpectedly, before any audio was saved.";
  }
  return `Recording stopped unexpectedly. Audio up to ${formatElapsed(audioMs)} was saved.`;
}

/**
 * "12 lines" / "1 line" / "No transcript yet".
 *
 * Zero gets words rather than a "0", because a meeting with no transcript is a
 * state the user may need to think about, not a quantity.
 */
export function formatLineCount(count: number): string {
  if (count === 0) return "No transcript yet";
  return count === 1 ? "1 line" : `${count} lines`;
}
