import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { OVERLAY_SETTING_LABEL, OverlaySetting } from "./OverlaySetting";

/** TUR-146: Settings' switch for the recording overlay. */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

async function show() {
  render(<OverlaySetting />);
  // The setting is read on mount.
  await act(async () => {});
  return screen.getByRole("switch", { name: OVERLAY_SETTING_LABEL });
}

describe("OverlaySetting", () => {
  test("is on by default", async () => {
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("shows what config.jsonc says", async () => {
    ipc.showRecordingOverlay.mockResolvedValueOnce(false);
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  test("switching saves it", async () => {
    const toggle = await show();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(ipc.setShowRecordingOverlay).toHaveBeenCalledWith(false);
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });
});
