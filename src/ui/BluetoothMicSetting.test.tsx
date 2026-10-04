import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { BLUETOOTH_MIC_LABEL, BluetoothMicSetting } from "./BluetoothMicSetting";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

async function show() {
  render(<BluetoothMicSetting />);
  // The setting is read on mount.
  await act(async () => {});
  return screen.getByRole("switch", { name: BLUETOOTH_MIC_LABEL });
}

describe("BluetoothMicSetting", () => {
  test("is on by default", async () => {
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("shows what config.jsonc says", async () => {
    ipc.builtinMicWithBluetooth.mockResolvedValueOnce(false);
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  test("switching saves it", async () => {
    const toggle = await show();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(ipc.setBuiltinMicWithBluetooth).toHaveBeenCalledWith(false);
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });
});
