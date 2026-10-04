import { act, render, screen, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { BLUETOOTH_MIC_LABEL } from "@/ui/BluetoothMicSetting";
import { Settings } from "./Settings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const RETENTION_TEXT = /^Audio is kept for 7 days\./;

async function show() {
  render(<Settings />);
  await act(async () => {});
}

describe("Settings: Audio section", () => {
  test("holds the audio retention row and the Bluetooth mic row", async () => {
    await show();
    const audio = screen.getByRole("region", { name: "Audio" });
    expect(await within(audio).findByText(RETENTION_TEXT)).toBeTruthy();
    expect(within(audio).getByRole("switch", { name: BLUETOOTH_MIC_LABEL })).toBeTruthy();
  });

  test("Files has neither row", async () => {
    await show();
    const files = screen.getByRole("region", { name: "Files" });
    expect(within(files).queryByText(RETENTION_TEXT)).toBeNull();
    expect(within(files).queryByRole("switch", { name: BLUETOOTH_MIC_LABEL })).toBeNull();
  });

  test("Audio comes right after Speech", async () => {
    await show();
    const titles = screen
      .getAllByRole("heading", { level: 2 })
      .map((heading) => heading.textContent);
    const speech = titles.indexOf("Speech");
    expect(speech).toBeGreaterThanOrEqual(0);
    expect(titles[speech + 1]).toBe("Audio");
  });
});
