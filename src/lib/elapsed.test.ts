import { describe, expect, it } from "vitest";
import { formatTimer, isPaused, recordedMs } from "./elapsed";

describe("recordedMs", () => {
  it("is the time since the start while recording", () => {
    expect(recordedMs({ startedAtMs: 1_000, pause: undefined }, 6_000)).toBe(5_000);
    expect(
      recordedMs({ startedAtMs: 1_000, pause: { pausedAtMs: null, pausedTotalMs: 0 } }, 6_000),
    ).toBe(5_000);
  });

  it("leaves every finished pause out", () => {
    const status = { startedAtMs: 0, pause: { pausedAtMs: null, pausedTotalMs: 4_000 } };
    expect(recordedMs(status, 10_000)).toBe(6_000);
  });

  it("freezes while paused, however long the pause runs", () => {
    const status = { startedAtMs: 0, pause: { pausedAtMs: 7_000, pausedTotalMs: 2_000 } };
    expect(recordedMs(status, 8_000)).toBe(5_000);
    expect(recordedMs(status, 3_600_000)).toBe(5_000);
  });

  it("is zero before a start, and never negative", () => {
    expect(recordedMs({ startedAtMs: null, pause: undefined }, 5_000)).toBe(0);
    expect(recordedMs({ startedAtMs: 9_000, pause: undefined }, 5_000)).toBe(0);
  });
});

describe("formatTimer", () => {
  it("is mm:ss under an hour", () => {
    expect(formatTimer(0)).toBe("00:00");
    expect(formatTimer(65_400)).toBe("01:05");
    expect(formatTimer(3_599_999)).toBe("59:59");
  });

  it("is h:mm:ss from an hour on", () => {
    expect(formatTimer(3_600_000)).toBe("1:00:00");
    expect(formatTimer(37_230_000)).toBe("10:20:30");
  });

  it("never shows a negative time", () => {
    expect(formatTimer(-5_000)).toBe("00:00");
  });
});

describe("isPaused", () => {
  it("is true only for a recording with an open pause", () => {
    const open = { pausedAtMs: 7_000, pausedTotalMs: 0 };
    expect(isPaused({ phase: "recording", pause: open })).toBe(true);
    expect(
      isPaused({ phase: "recording", pause: { pausedAtMs: null, pausedTotalMs: 3_000 } }),
    ).toBe(false);
    expect(isPaused({ phase: "recording", pause: undefined })).toBe(false);
    expect(isPaused({ phase: "stopping", pause: open })).toBe(false);
  });
});
