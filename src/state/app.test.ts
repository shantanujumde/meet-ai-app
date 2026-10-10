import { beforeEach, describe, expect, it, vi } from "vitest";
import { meetingSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { useAppStore } from "./app";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const INITIAL = useAppStore.getState();
beforeEach(() => useAppStore.setState(INITIAL, true));

const failure = { domain: "app", kind: "unexpected", message: "boom" };

describe("app store: meetings", () => {
  it("loads the list and clears the loading flag", async () => {
    const list = { meetings: [meetingSummary()], unreadable: [] };
    ipc.listMeetings.mockResolvedValue(list as never);
    await useAppStore.getState().loadMeetings();
    expect(useAppStore.getState().meetings).toEqual(list);
    expect(useAppStore.getState().meetingsLoading).toBe(false);
  });

  it("records a failure, and a silent refresh keeps the old list", async () => {
    ipc.listMeetings.mockRejectedValueOnce(failure);
    await useAppStore.getState().loadMeetings();
    expect(useAppStore.getState().meetingsError?.message).toBe("boom");

    const kept = { meetings: [meetingSummary()], unreadable: [] };
    useAppStore.setState({ meetings: kept as never, meetingsError: null });
    ipc.listMeetings.mockRejectedValueOnce(failure);
    await useAppStore.getState().loadMeetings({ silent: true });
    expect(useAppStore.getState().meetings).toEqual(kept);
    expect(useAppStore.getState().meetingsError).toBeNull();
  });
});

describe("app store: permission", () => {
  it("reports a failed check as unknown, never denied", async () => {
    ipc.measurePermission.mockRejectedValueOnce(failure);
    await useAppStore.getState().loadPermission();
    const { permission, permissionLoading } = useAppStore.getState();
    expect(permission?.state).toBe("unknown");
    expect(permission?.denied).toEqual([]);
    expect(permissionLoading).toBe(false);
  });

  it("a silent check never replaces a measured answer", async () => {
    const measured = { state: "granted", measured: true, detail: "", denied: [] };
    useAppStore.setState({ permission: measured as never });
    await useAppStore.getState().loadPermission({ silent: true });
    expect(useAppStore.getState().permission).toEqual(measured);
  });
});

describe("app store: onboarding", () => {
  it("treats an unreadable flag as not onboarded", async () => {
    ipc.onboardingState.mockRejectedValueOnce(failure);
    await useAppStore.getState().loadOnboarding();
    expect(useAppStore.getState().onboarding).toEqual({ completedAt: null });
  });

  it("keeps the user out of the wizard when the folder pointer is damaged", async () => {
    ipc.onboardingState.mockRejectedValueOnce({
      domain: "app",
      kind: "root-pointer-unreadable",
      message: "x",
    });
    await useAppStore.getState().loadOnboarding();
    expect(useAppStore.getState().onboarding).toBeNull();
    expect(useAppStore.getState().onboardingLoading).toBe(false);
  });

  it("lets the user through when the done flag cannot be saved", async () => {
    ipc.completeOnboarding.mockRejectedValueOnce(failure);
    await useAppStore.getState().finishOnboarding();
    expect(useAppStore.getState().onboarding?.completedAt).not.toBeNull();
  });

  it("keeps a failed restart out of the meeting list error", async () => {
    ipc.resetOnboarding.mockRejectedValueOnce(failure);
    await useAppStore.getState().restartOnboarding();
    expect(useAppStore.getState().onboardingError?.message).toBe("boom");
    expect(useAppStore.getState().meetingsError).toBeNull();
  });
});
