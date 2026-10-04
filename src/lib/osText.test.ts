import { platform } from "@tauri-apps/plugin-os";
import { afterEach, expect, test, vi } from "vitest";
import { currentOs, osText } from "./osText";

vi.mock("@tauri-apps/plugin-os", () => ({ platform: vi.fn() }));

afterEach(() => {
  vi.mocked(platform).mockReset();
});

test("names each OS's own apps", () => {
  expect(osText("settings", "macos")).toBe("System Settings");
  expect(osText("settings", "windows")).toBe("Settings");
  expect(osText("fileManager", "windows")).toBe("File Explorer");
  expect(osText("fileManager", "linux")).toBe("file manager");
  expect(osText("terminal", "windows")).toBe("PowerShell");
  expect(osText("terminal", "macos")).toBe("Terminal");
  expect(osText("terminal", "linux")).toBe("Terminal");
  expect(osText("settings", "linux")).toBe("Settings");
  expect(osText("fileManager", "macos")).toBe("Finder");
});

test("reads the OS from the plugin", () => {
  vi.mocked(platform).mockReturnValue("windows");
  expect(currentOs()).toBe("windows");
  expect(osText("settings")).toBe("Settings");
  vi.mocked(platform).mockReturnValue("linux");
  expect(currentOs()).toBe("linux");
});

test("is macOS without the plugin or on another OS", () => {
  vi.mocked(platform).mockImplementation(() => {
    throw new TypeError("no plugin");
  });
  expect(currentOs()).toBe("macos");
  vi.mocked(platform).mockReturnValue("freebsd");
  expect(currentOs()).toBe("macos");
});
