import { describe, expect, test } from "vitest";
import {
  describeInterruption,
  formatBytes,
  formatDuration,
  formatElapsed,
  formatLineCount,
  formatRelativeDate,
  timestampToMs,
} from "./format";

describe("model sizes", () => {
  test("the pinned models read as the numbers their source publishes", () => {
    // The two sizes from crates/stt/src/model.rs. SPEC §2.4 says "~1.6 GB" for
    // turbo, which amendment A4 corrects to 574 MB — so this is also the test
    // that the screen is quoting the catalogue and not a stale document.
    expect(formatBytes(190_098_681)).toBe("190 MB");
    expect(formatBytes(574_041_195)).toBe("574 MB");
  });

  test("decimal units, because that is what macOS shows", () => {
    // Binary units would render the 190,098,681-byte model as "181 MB" and
    // make it look wrong next to every other source for the same file.
    expect(formatBytes(1_000_000)).toBe("1 MB");
    expect(formatBytes(2_500_000_000)).toBe("2.5 GB");
  });

  test("nonsense in gets a dash, not NaN", () => {
    expect(formatBytes(Number.NaN)).toBe("unknown size");
    expect(formatBytes(-1)).toBe("unknown size");
  });
});

describe("the recording timer", () => {
  test("counts in HH:MM:SS", () => {
    expect(formatElapsed(0)).toBe("00:00:00");
    expect(formatElapsed(61_000)).toBe("00:01:01");
    expect(formatElapsed(45 * 60 * 1000)).toBe("00:45:00");
    expect(formatElapsed(3 * 3600 * 1000 + 7000)).toBe("03:00:07");
  });

  test("a clock that went backwards shows zero rather than a negative time", () => {
    expect(formatElapsed(-5000)).toBe("00:00:00");
  });
});

describe("meeting dates", () => {
  const today = new Date(2026, 8, 27); // 27 Sep 2026, local

  test("today and yesterday get words", () => {
    expect(formatRelativeDate("2026-09-27", today)).toBe("Today");
    expect(formatRelativeDate("2026-09-26", today)).toBe("Yesterday");
  });

  test("within the last week gets a weekday", () => {
    expect(formatRelativeDate("2026-09-23", today)).toBe("Wednesday");
  });

  test("older dates get a date, with the year only when it differs", () => {
    expect(formatRelativeDate("2026-09-01", today)).not.toMatch(/2026/);
    expect(formatRelativeDate("2025-09-01", today)).toMatch(/2025/);
  });

  test("a date is parsed in local time, not UTC", () => {
    // `new Date("2026-09-27")` is UTC midnight, which is the 26th anywhere west
    // of Greenwich — so a meeting recorded this morning would read "Yesterday".
    expect(formatRelativeDate("2026-09-27", today)).toBe("Today");
  });

  test("a folder someone renamed by hand still renders", () => {
    expect(formatRelativeDate(null)).toBe("No date");
    expect(formatRelativeDate("not-a-date", today)).toBe("not-a-date");
  });
});

describe("transcript line counts", () => {
  test("zero is a sentence, not a zero", () => {
    expect(formatLineCount(0)).toBe("No transcript yet");
  });

  test("one is singular", () => {
    expect(formatLineCount(1)).toBe("1 line");
    expect(formatLineCount(42)).toBe("42 lines");
  });
});

describe("interrupted meetings (TUR-97)", () => {
  test("say how far the saved audio goes, in the transcript's own clock format", () => {
    // 16.25 s — the v0.3.0 kill on the dev machine, once its header is fixed.
    expect(describeInterruption(16_253)).toBe(
      "Recording stopped unexpectedly. Audio up to 00:00:16 was saved.",
    );
    expect(describeInterruption(3_725_000)).toMatch(/up to 01:02:05 was saved/);
  });

  test("never claim audio was saved when none a player can reach was", () => {
    // A header still declaring 0 bytes, or a folder with no WAV at all.
    expect(describeInterruption(0)).toBe(
      "Recording stopped unexpectedly, before any audio was saved.",
    );
    expect(describeInterruption(null)).toMatch(/before any audio was saved/);
  });
});

describe("meeting length in the header (TUR-81)", () => {
  test("rounds to the minute, and says hours past an hour", () => {
    expect(formatDuration(0)).toBe("< 1 min");
    expect(formatDuration(29_000)).toBe("< 1 min");
    expect(formatDuration(50 * 60_000)).toBe("50 min");
    expect(formatDuration(65 * 60_000)).toBe("1 h 5 min");
    expect(formatDuration(120 * 60_000 + 10_000)).toBe("2 h");
  });

  test("reads a transcript timestamp, and nothing else", () => {
    expect(timestampToMs("00:50:04")).toBe((50 * 60 + 4) * 1000);
    expect(timestampToMs("01:00:00")).toBe(3_600_000);
    expect(timestampToMs(null)).toBeNull();
    expect(timestampToMs("50 min")).toBeNull();
  });
});
