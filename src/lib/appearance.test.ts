import { describe, expect, it } from "vitest";
import { applyAppearance, resolveTheme } from "./appearance";

describe("resolveTheme", () => {
  it("follows the OS under System", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("forces Light or Dark whatever the OS says", () => {
    for (const systemDark of [true, false]) {
      expect(resolveTheme("light", systemDark)).toBe("light");
      expect(resolveTheme("dark", systemDark)).toBe("dark");
    }
  });
});

describe("applyAppearance", () => {
  it("sets the resolved theme and marks glass off", () => {
    const root = document.createElement("html");
    applyAppearance({ theme: "system", glass: false }, root, true);
    expect(root.dataset.theme).toBe("dark");
    expect(root.dataset.glass).toBe("off");
  });

  it("drops the glass mark when glass comes back on", () => {
    const root = document.createElement("html");
    applyAppearance({ theme: "light", glass: false }, root, true);
    applyAppearance({ theme: "light", glass: true }, root, true);
    expect(root.dataset.theme).toBe("light");
    expect(root.dataset.glass).toBeUndefined();
  });
});
