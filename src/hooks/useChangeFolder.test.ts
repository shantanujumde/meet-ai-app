/**
 * TUR-149: the change-folder flow when the meetings-folder pointer is
 * damaged, and the onboarding load that must not send the user back through
 * setup over it.
 */

import { ask, open } from "@tauri-apps/plugin-dialog";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { ROOT_POINTER_UNREADABLE } from "@/ipc/errors";
import type { UiError } from "@/ipc/types";
import { useAppStore } from "@/state/app";
import { ipc } from "@/test/ipcMock";
import { useChangeFolder } from "./useChangeFolder";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(), open: vi.fn() }));

const confirm = vi.mocked(ask);
const pickFolder = vi.mocked(open);

const LOST: UiError = {
  domain: "app",
  kind: ROOT_POINTER_UNREADABLE,
  message: "The file that remembers where your meetings folder is (root.json) is damaged.",
};

beforeEach(() => {
  confirm.mockReset();
  pickFolder.mockReset();
  pickFolder.mockResolvedValue("/Volumes/Data/Meetings");
  confirm.mockResolvedValue(true);
});

describe("useChangeFolder", () => {
  test("a normal move says the meetings move from the current folder", async () => {
    useAppStore.setState({
      meetings: { root: "/Users/me/Meetings", rootExists: true, meetings: [] },
      onboarding: { completedAt: "2026-10-01T09:00:00Z" },
    });
    const { result } = renderHook(() => useChangeFolder());

    await act(() => result.current.pick());

    expect(confirm.mock.calls[0]?.[0]).toContain(
      "move from /Users/me/Meetings to /Volumes/Data/Meetings",
    );
    expect(confirm.mock.calls[0]?.[1]).toMatchObject({ okLabel: "Move meetings" });
    expect(ipc.changeMeetingsFolder).toHaveBeenCalledWith("/Volumes/Data/Meetings");
    expect(ipc.onboardingState).not.toHaveBeenCalled();
  });

  test("with the pointer damaged, picking a folder only points meet-ai at it", async () => {
    useAppStore.setState({ meetings: null, meetingsError: LOST, onboarding: null });
    ipc.onboardingState.mockResolvedValue({ completedAt: "2026-10-01T09:00:00Z" });
    const { result } = renderHook(() => useChangeFolder());

    await act(() => result.current.pick());

    const [text, options] = confirm.mock.calls[0] ?? [];
    expect(text).toContain("keep your meetings in /Volumes/Data/Meetings");
    expect(text).not.toContain("move from");
    expect(options).toMatchObject({ title: "Use this meetings folder?" });
    expect(ipc.changeMeetingsFolder).toHaveBeenCalledWith("/Volumes/Data/Meetings");
    // Whether setup was done is known again, from the folder just picked.
    expect(useAppStore.getState().onboarding).toEqual({ completedAt: "2026-10-01T09:00:00Z" });
    expect(useAppStore.getState().meetingsError).toBeNull();
  });
});

describe("loadOnboarding", () => {
  test("a damaged pointer does not send the user back through setup", async () => {
    ipc.onboardingState.mockRejectedValue(LOST);

    await useAppStore.getState().loadOnboarding();

    // Null is "unknown": App.tsx only redirects to setup on `completedAt: null`.
    expect(useAppStore.getState().onboarding).toBeNull();
    expect(useAppStore.getState().onboardingLoading).toBe(false);
  });

  test("any other unreadable flag still means not onboarded", async () => {
    ipc.onboardingState.mockRejectedValue({ domain: "app", kind: "io", message: "disk gone" });

    await useAppStore.getState().loadOnboarding();

    expect(useAppStore.getState().onboarding).toEqual({ completedAt: null });
  });

  // TUR-170: the confirm dialog itself failing shows an error, and moves nothing.
  test("a confirm dialog that fails shows the error instead of rejecting", async () => {
    useAppStore.setState({
      meetings: { root: "/Users/me/Meetings", rootExists: true, meetings: [] },
    });
    ipc.changeMeetingsFolder.mockClear();
    confirm.mockRejectedValue(new Error("dialog plugin unavailable"));
    const { result } = renderHook(() => useChangeFolder());

    await act(() => result.current.pick());

    expect(result.current.error?.message).toContain("dialog plugin unavailable");
    expect(result.current.busy).toBe(false);
    expect(ipc.changeMeetingsFolder).not.toHaveBeenCalled();
  });
});
