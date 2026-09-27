/**
 * Formatting helpers shared by the screens.
 *
 * Pure functions with no React in them, so they are testable on their own —
 * which matters, because "190 MB" versus "181 MiB" and "Today" versus a raw
 * date are exactly the details that make an interface feel hand-made or not.
 */

/**
 * Bytes as a person would say them.
 *
 * Decimal units (MB = 10^6), which is what macOS itself shows and what the
 * model's own download page says. Binary units would render the pinned
 * 190,098,681-byte model as "181 MB" and make the number look wrong next to
 * every other source.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
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
 * "12 lines" / "1 line" / "No transcript yet".
 *
 * Zero gets words rather than a "0", because a meeting with no transcript is a
 * state the user may need to think about, not a quantity.
 */
export function formatLineCount(count: number): string {
  if (count === 0) return "No transcript yet";
  return count === 1 ? "1 line" : `${count} lines`;
}
