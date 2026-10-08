import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { NEVER_DETECT_LABEL, NeverDetectList } from "./NeverDetectList";
import { NotificationSettings } from "./NotificationSettings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

async function show(onError = vi.fn()) {
  render(<NeverDetectList onError={onError} />);
  await act(async () => {});
  return onError;
}

describe("NeverDetectList", () => {
  test("with no apps, says how one gets there", async () => {
    await show();
    expect(screen.getByText(NEVER_DETECT_LABEL)).toBeTruthy();
    expect(screen.getByText(/Never for, in a call prompt's ⋯ menu, adds one/)).toBeTruthy();
    expect(screen.queryByRole("list", { name: NEVER_DETECT_LABEL })).toBeNull();
  });

  test("lists the saved apps, each with its own Remove", async () => {
    ipc.neverDetectApps.mockResolvedValueOnce(["WhatsApp", "Google Chrome"]);
    await show();
    const list = screen.getByRole("list", { name: NEVER_DETECT_LABEL });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      "WhatsAppRemove",
      "Google ChromeRemove",
    ]);
    expect(
      screen.getByRole("button", { name: `Remove WhatsApp from ${NEVER_DETECT_LABEL}` }),
    ).toBeTruthy();
  });

  test("Remove saves the list without that app and shows what was saved", async () => {
    ipc.neverDetectApps.mockResolvedValueOnce(["WhatsApp", "Google Chrome"]);
    await show();
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: `Remove WhatsApp from ${NEVER_DETECT_LABEL}` }),
      );
    });
    expect(ipc.setNeverDetectApps).toHaveBeenCalledWith(["Google Chrome"]);
    expect(screen.queryByText("WhatsApp")).toBeNull();
    expect(screen.getByText("Google Chrome")).toBeTruthy();
  });

  test("a failed save keeps the app and reports why", async () => {
    ipc.neverDetectApps.mockResolvedValueOnce(["WhatsApp"]);
    const error = { domain: "app", kind: "io", message: "The meetings folder is moving." };
    ipc.setNeverDetectApps.mockRejectedValueOnce(error);
    const onError = await show();
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: `Remove WhatsApp from ${NEVER_DETECT_LABEL}` }),
      );
    });
    expect(screen.getByText("WhatsApp")).toBeTruthy();
    expect(onError).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "io" }));
  });

  test("sits in Settings → Notifications, where its errors show", async () => {
    ipc.neverDetectApps.mockResolvedValueOnce(["WhatsApp"]);
    ipc.setNeverDetectApps.mockRejectedValueOnce({
      domain: "app",
      kind: "io",
      message: "The meetings folder is moving.",
    });
    render(<NotificationSettings />);
    await act(async () => {});
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: `Remove WhatsApp from ${NEVER_DETECT_LABEL}` }),
      );
    });
    expect(screen.getByText(/The meetings folder is moving/)).toBeTruthy();
  });
});
