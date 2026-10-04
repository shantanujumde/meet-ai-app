import { waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { useAppearanceStore, watchAppearance } from "./appearance";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

describe("appearance store", () => {
  it("reads the saved choice and puts it on <html>", async () => {
    ipc.appearanceSettings.mockResolvedValue({ theme: "dark", glass: false });
    const root = document.createElement("html");
    const stop = watchAppearance(root);
    await waitFor(() => expect(root.dataset.theme).toBe("dark"));
    expect(root.dataset.glass).toBe("off");
    expect(useAppearanceStore.getState().loaded).toBe(true);
    stop();
  });

  it("saves a change and applies it at once", async () => {
    const root = document.createElement("html");
    const stop = watchAppearance(root);
    await waitFor(() => expect(useAppearanceStore.getState().loaded).toBe(true));
    await useAppearanceStore.getState().save({ theme: "light" });
    expect(ipc.setAppearance).toHaveBeenCalledWith({ theme: "light", glass: true });
    expect(root.dataset.theme).toBe("light");
    stop();
  });

  it("goes back to the old look when the save fails", async () => {
    ipc.setAppearance.mockRejectedValue(new Error("disk full"));
    await useAppearanceStore.getState().save({ glass: false });
    const state = useAppearanceStore.getState();
    expect(state.appearance.glass).toBe(true);
    expect(state.error?.message).toBe("disk full");
  });
});
