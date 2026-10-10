import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentChoice } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { useAgentSetup } from "./useAgentSetup";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const claude = (model: string): AgentChoice => ({
  harness: "claude-code",
  model,
  binaryPath: null,
});

describe("useAgentSetup", () => {
  it("only the newest of two saves in flight lands", async () => {
    const { result } = renderHook(() => useAgentSetup());
    await waitFor(() => expect(result.current.detecting).toBe(false));
    expect(result.current.choice).not.toBeNull();

    let releaseOld: (choice: AgentChoice) => void = () => {};
    ipc.saveAgentChoice.mockImplementationOnce(
      () => new Promise<AgentChoice>((resolve) => (releaseOld = resolve)),
    );
    ipc.saveAgentChoice.mockImplementationOnce(async (choice) => choice);

    act(() => result.current.setModel("sonnet"));
    await waitFor(() => expect(ipc.saveAgentChoice).toHaveBeenCalledTimes(1));
    act(() => result.current.setModel("opus"));
    await waitFor(() => expect(result.current.choice?.model).toBe("opus"));
    await waitFor(() => expect(ipc.saveAgentChoice).toHaveBeenCalledTimes(2));

    await act(async () => releaseOld(claude("sonnet")));
    expect(result.current.choice?.model).toBe("opus");
  });

  it("puts the previous choice back and says why when a save fails", async () => {
    const { result } = renderHook(() => useAgentSetup());
    await waitFor(() => expect(result.current.detecting).toBe(false));
    const before = result.current.choice;

    ipc.saveAgentChoice.mockRejectedValueOnce({
      domain: "app",
      kind: "unexpected",
      message: "read-only",
    });
    act(() => result.current.setModel("opus"));
    await waitFor(() => expect(result.current.saveError?.message).toBe("read-only"));
    expect(result.current.choice).toEqual(before);
  });
});
