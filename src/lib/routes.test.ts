import { describe, expect, test } from "vitest";
import { briefPath, navigationPath, SETTINGS } from "./routes";

describe("navigationPath", () => {
  test("the menu bar's Open brief lands on that meeting's brief", () => {
    expect(navigationPath({ to: "brief", title: "R&D sync #3" })).toBe(briefPath("R&D sync #3"));
  });

  test("Calendar not connected lands on Settings", () => {
    expect(navigationPath({ to: "settings" })).toBe(SETTINGS);
  });
});
