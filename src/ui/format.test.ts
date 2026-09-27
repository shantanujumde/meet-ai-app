import { describe, expect, test } from "vitest";
import { formatBytes, formatElapsed, formatLineCount, formatRelativeDate } from "./format";

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
    expect(formatBytes(Number.NaN)).toBe("—");
    expect(formatBytes(-1)).toBe("—");
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
