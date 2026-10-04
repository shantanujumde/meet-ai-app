import { describe, expect, test } from "vitest";
import { applyOsAttribute } from "./osAttribute";

describe("applyOsAttribute", () => {
  test.each(["macos", "windows", "linux"] as const)("sets data-os=%s", (os) => {
    const root = document.createElement("html");
    applyOsAttribute(root, os);
    expect(root.getAttribute("data-os")).toBe(os);
  });

  test("defaults to macOS outside the app", () => {
    const root = document.createElement("html");
    applyOsAttribute(root);
    expect(root.dataset.os).toBe("macos");
  });
});
