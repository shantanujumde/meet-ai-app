/**
 * TUR-178: the one "read on mount, then save" pattern behind the Settings
 * cards.
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { UiError } from "@/ipc/types";
import { useIpcValue, useSavedSetting } from "./useIpcValue";

const REFUSED: UiError = { domain: "app", kind: "config-write", message: "Could not save." };

/** A promise and the functions that settle it. */
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

describe("useIpcValue", () => {
  test("shows what load answers", async () => {
    const load = vi.fn(async () => 5);
    const { result } = renderHook(() => useIpcValue(load));
    expect(result.current.value).toBeNull();
    await waitFor(() => expect(result.current.value).toBe(5));
    expect(result.current.error).toBeNull();
    expect(load).toHaveBeenCalledTimes(1);
  });

  test("a failed load is a UiError, and the value stays empty", async () => {
    const load = vi.fn(async () => {
      throw REFUSED;
    });
    const { result } = renderHook(() => useIpcValue(load));
    await waitFor(() => expect(result.current.error).toEqual(REFUSED));
    expect(result.current.value).toBeNull();
  });

  test("an answer after unmount is dropped", async () => {
    const answer = deferred<number>();
    const load = vi.fn(() => answer.promise);
    const { result, unmount } = renderHook(() => useIpcValue(load));
    unmount();
    await act(async () => {
      answer.resolve(7);
      await answer.promise;
    });
    expect(result.current.value).toBeNull();
  });

  test("a function value is stored, not called", async () => {
    const fn = () => "inner";
    const load = vi.fn(async () => fn);
    const { result } = renderHook(() => useIpcValue(load));
    await waitFor(() => expect(result.current.value).toBe(fn));
  });
});

describe("useSavedSetting", () => {
  test("shows what save answers was saved, busy while it runs", async () => {
    const saving = deferred<boolean>();
    const load = vi.fn(async () => false);
    const save = vi.fn(() => saving.promise);
    const { result } = renderHook(() => useSavedSetting(load, save));
    await waitFor(() => expect(result.current.value).toBe(false));

    let done: Promise<void> = Promise.resolve();
    act(() => {
      done = result.current.change(true);
    });
    expect(result.current.busy).toBe(true);
    expect(save).toHaveBeenCalledWith(true);

    await act(async () => {
      saving.resolve(true);
      await done;
    });
    expect(result.current.busy).toBe(false);
    expect(result.current.value).toBe(true);
  });

  test("a failed save keeps the old value and says why", async () => {
    const load = vi.fn(async () => false);
    const save = vi.fn(async () => {
      throw REFUSED;
    });
    const { result } = renderHook(() => useSavedSetting(load, save));
    await waitFor(() => expect(result.current.value).toBe(false));

    await act(async () => {
      await result.current.change(true);
    });
    expect(result.current.value).toBe(false);
    expect(result.current.error).toEqual(REFUSED);
    expect(result.current.busy).toBe(false);
  });
});
