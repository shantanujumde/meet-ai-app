import { describe, expect, test } from "vitest";
import { applyOsAttribute } from "./osAttribute";
import { osText } from "./osText";

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

describe("OS words", () => {
  test.each([
    ["macos", "Finder", "Terminal", "System Settings"],
    ["windows", "File Explorer", "PowerShell", "Settings"],
    ["linux", "file manager", "Terminal", "Settings"],
  ] as const)("%s", (os, files, terminal, settings) => {
    expect(osText("fileManager", os)).toBe(files);
    expect(osText("terminal", os)).toBe(terminal);
    expect(osText("settings", os)).toBe(settings);
  });
});
