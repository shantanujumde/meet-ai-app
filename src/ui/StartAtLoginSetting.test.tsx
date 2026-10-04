import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { START_AT_LOGIN_LABEL, StartAtLoginSetting } from "./StartAtLoginSetting";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

async function show() {
  render(<StartAtLoginSetting />);
  // The setting is read on mount.
  await act(async () => {});
  return screen.getByRole("switch", { name: START_AT_LOGIN_LABEL });
}

describe("StartAtLoginSetting", () => {
  test("is off by default", async () => {
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).not.toBeDisabled();
  });

  test("shows what the OS says", async () => {
    ipc.startAtLogin.mockResolvedValueOnce(true);
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("switching saves it and shows what was saved", async () => {
    const toggle = await show();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(ipc.setStartAtLogin).toHaveBeenCalledWith(true);
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("a failed save keeps the old state", async () => {
    ipc.setStartAtLogin.mockRejectedValueOnce({
      domain: "app",
      kind: "io",
      message: "Could not change the login item.",
    });
    const toggle = await show();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(/Could not change the login item/)).toBeTruthy();
  });
});
