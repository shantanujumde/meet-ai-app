import { describe, expect, it } from "vitest";
import { furthestAfter } from "./useHistoryArrows";

describe("furthestAfter", () => {
  it("drops everything ahead on a push or a replace", () => {
    expect(furthestAfter("PUSH", 2, 5)).toBe(2);
    expect(furthestAfter("REPLACE", 3, 5)).toBe(3);
  });

  it("keeps what is ahead when going back or forward", () => {
    expect(furthestAfter("POP", 1, 4)).toBe(4);
    expect(furthestAfter("POP", 4, 3)).toBe(4);
  });
});
