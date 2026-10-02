import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { copyText } from "./clipboard";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(async () => {}) }));

const browserWrite = vi.fn(async (_text: string) => {});

function stubBrowserClipboard() {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: browserWrite },
  });
}

describe("copyText", () => {
  afterEach(() => {
    vi.mocked(writeText).mockReset();
    browserWrite.mockReset();
  });

  test("goes through the Tauri plugin inside the app", async () => {
    stubBrowserClipboard();
    ipc.hasBackend.mockReturnValue(true);
    await copyText("hello");
    expect(writeText).toHaveBeenCalledWith("hello");
    expect(browserWrite).not.toHaveBeenCalled();
  });

  test("falls back to the browser clipboard without a backend", async () => {
    stubBrowserClipboard();
    ipc.hasBackend.mockReturnValue(false);
    await copyText("hello");
    expect(browserWrite).toHaveBeenCalledWith("hello");
    expect(writeText).not.toHaveBeenCalled();
  });

  test("a refusal is thrown to the caller", async () => {
    ipc.hasBackend.mockReturnValue(true);
    vi.mocked(writeText).mockRejectedValueOnce(new Error("denied"));
    await expect(copyText("hello")).rejects.toThrow("denied");
  });
});
