import { describe, expect, it } from "vitest";
import { terminalName } from "./terminalName";

describe("terminalName", () => {
  it("is PowerShell on Windows", () => {
    expect(terminalName("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")).toBe(
      "PowerShell",
    );
  });

  it("is Terminal on macOS and Linux", () => {
    expect(
      terminalName("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15"),
    ).toBe("Terminal");
    expect(terminalName("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")).toBe("Terminal");
  });
});
