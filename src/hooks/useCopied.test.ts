/**
 * TUR-178: the one "Copied" state. Its timer restarts on a second copy and
 * never outlives the component.
 */

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { copyText } from "@/lib/clipboard";
import { COPIED_RESET_MS } from "@/lib/constants";
import { useCopied } from "./useCopied";

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async () => {}) }));

const clipboard = vi.mocked(copyText);

beforeEach(() => {
  vi.useFakeTimers();
  clipboard.mockReset();
  clipboard.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useCopied", () => {
  test("says Copied for COPIED_RESET_MS, then goes back", async () => {
    const { result } = renderHook(() => useCopied());
    let copied = false;
    await act(async () => {
      copied = await result.current.copy("hello");
    });
    expect(copied).toBe(true);
    expect(clipboard).toHaveBeenCalledWith("hello");
    expect(result.current.copied).toBe(true);

    act(() => vi.advanceTimersByTime(COPIED_RESET_MS));
    expect(result.current.copied).toBe(false);
  });

  test("a second copy restarts the time instead of being cut short", async () => {
    const { result } = renderHook(() => useCopied());
    await act(async () => {
      await result.current.copy("one");
    });
    act(() => vi.advanceTimersByTime(COPIED_RESET_MS - 500));
    await act(async () => {
      await result.current.copy("two");
    });

    // The first copy's timer would have fired here.
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current.copied).toBe(true);
    expect(vi.getTimerCount()).toBe(1);

    act(() => vi.advanceTimersByTime(COPIED_RESET_MS));
    expect(result.current.copied).toBe(false);
  });

  test("no timer outlives the component", async () => {
    const { result, unmount } = renderHook(() => useCopied());
    await act(async () => {
      await result.current.copy("hello");
    });
    expect(vi.getTimerCount()).toBe(1);

    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  test("a refused clipboard reports false and never says Copied", async () => {
    clipboard.mockRejectedValueOnce(new Error("denied"));
    const { result } = renderHook(() => useCopied());
    let copied = true;
    await act(async () => {
      copied = await result.current.copy("hello");
    });
    expect(copied).toBe(false);
    expect(result.current.copied).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  test("reset drops a Copied still on screen", async () => {
    const { result } = renderHook(() => useCopied());
    await act(async () => {
      await result.current.copy("hello");
    });
    act(() => result.current.reset());
    expect(result.current.copied).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });
});
