import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { MENU_BAR_COUNTDOWN_LABEL, MenuBarCountdownSetting } from "./MenuBarCountdownSetting";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

async function show() {
  render(<MenuBarCountdownSetting />);
  // The setting is read on mount.
  await act(async () => {});
  return screen.getByRole("switch", { name: MENU_BAR_COUNTDOWN_LABEL });
}

describe("MenuBarCountdownSetting", () => {
  test("is off by default: the icon alone", async () => {
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).not.toBeDisabled();
  });

  test("shows what config.jsonc says", async () => {
    ipc.menuBarCountdown.mockResolvedValueOnce(true);
    const toggle = await show();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("switching saves it and shows what was saved", async () => {
    const toggle = await show();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(ipc.setMenuBarCountdown).toHaveBeenCalledWith(true);
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  test("a failed save keeps the old state", async () => {
    ipc.setMenuBarCountdown.mockRejectedValueOnce({
      domain: "app",
      kind: "io",
      message: "The meetings folder is moving.",
    });
    const toggle = await show();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(/The meetings folder is moving/)).toBeTruthy();
  });
});
