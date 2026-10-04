import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { useAppearanceStore, watchAppearance } from "@/state/appearance";
import { ipc } from "@/test/ipcMock";
import { AppearanceSettings, GLASS_SETTING_LABEL } from "./AppearanceSettings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

/** Settings → Appearance (TUR-102), with the store applying to a throwaway <html>. */
async function show() {
  const root = document.createElement("html");
  const stop = watchAppearance(root);
  render(<AppearanceSettings />);
  await act(async () => {});
  return { root, stop };
}

describe("Settings: Appearance", () => {
  test("shows the saved theme as the chosen tile and the glass switch on", async () => {
    ipc.appearanceSettings.mockResolvedValue({ theme: "dark", glass: true });
    const { stop } = await show();
    const section = screen.getByRole("region", { name: "Appearance" });
    const tiles = within(section).getByRole("radiogroup", { name: "Colours" });
    expect(within(tiles).getByRole("radio", { name: "Dark" })).toBeChecked();
    expect(within(tiles).getByRole("radio", { name: "Light" })).not.toBeChecked();
    expect(within(tiles).getByRole("radio", { name: "System" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: GLASS_SETTING_LABEL })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    stop();
  });

  test("picking Light saves it and the window turns light at once", async () => {
    ipc.appearanceSettings.mockResolvedValue({ theme: "dark", glass: true });
    const { root, stop } = await show();
    expect(root.dataset.theme).toBe("dark");
    await act(async () => {
      fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    });
    expect(ipc.setAppearance).toHaveBeenCalledWith({ theme: "light", glass: true });
    expect(root.dataset.theme).toBe("light");
    expect(screen.getByRole("radio", { name: "Light" })).toBeChecked();
    stop();
  });

  test("turning glass off saves it and marks the window solid", async () => {
    const { root, stop } = await show();
    await act(async () => {
      fireEvent.click(screen.getByRole("switch", { name: GLASS_SETTING_LABEL }));
    });
    expect(ipc.setAppearance).toHaveBeenCalledWith({ theme: "system", glass: false });
    expect(root.dataset.glass).toBe("off");
    expect(useAppearanceStore.getState().appearance.glass).toBe(false);
    stop();
  });

  test("a failed save puts the old look back and says why", async () => {
    ipc.setAppearance.mockRejectedValueOnce({
      domain: "app",
      kind: "io",
      message: "The meetings folder is moving.",
    });
    const { root, stop } = await show();
    await act(async () => {
      fireEvent.click(screen.getByRole("radio", { name: "Dark" }));
    });
    expect(screen.getByRole("radio", { name: "System" })).toBeChecked();
    expect(root.dataset.theme).toBe("light");
    expect(screen.getByRole("alert")).toHaveTextContent("The meetings folder is moving.");
    stop();
  });
});
