/**
 * TUR-170: engine, model and language saves land in order. Only the newest
 * save's answer is shown, so an older answer that lands last cannot
 * overwrite or revert a newer pick.
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { EngineChoices } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import { useSpeech } from "./useSpeech";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

/** A promise and the function that settles it, for answers that land out of order. */
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function loaded() {
  const hook = renderHook(() => useSpeech());
  await waitFor(() => expect(hook.result.current.choices).not.toBeNull());
  return hook;
}

describe("useSpeech saves", () => {
  let base: EngineChoices;

  beforeEach(async () => {
    base = await ipc.engineChoices();
    ipc.setTranscription.mockReset();
    ipc.setSpokenLanguage.mockReset();
  });

  test("an older answer landing last does not overwrite the newer pick", async () => {
    const { result } = await loaded();
    const first = deferred<EngineChoices>();
    const second = deferred<EngineChoices>();
    ipc.setTranscription.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    act(() => result.current.pickEngine("apple-speech"));
    act(() => result.current.pickModel("small"));
    // The second pick starts from the first, not from a render-old closure.
    expect(ipc.setTranscription).toHaveBeenLastCalledWith("apple-speech", "small");

    await act(async () => second.resolve({ ...base, engine: "apple-speech", model: "small" }));
    await act(async () => first.resolve({ ...base, engine: "apple-speech", model: base.model }));

    expect(result.current.choices?.model).toBe("small");
    expect(result.current.choices?.engine).toBe("apple-speech");
  });

  test("an older save that fails last does not revert the newer pick", async () => {
    const { result } = await loaded();
    const first = deferred<EngineChoices>();
    const second = deferred<EngineChoices>();
    ipc.setTranscription.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    act(() => result.current.pickEngine("apple-speech"));
    act(() => result.current.pickModel("small"));
    await act(async () => second.resolve({ ...base, engine: "apple-speech", model: "small" }));
    await act(async () => first.reject({ domain: "app", kind: "io", message: "disk" }));

    expect(result.current.choices?.model).toBe("small");
    expect(result.current.saveError).toBeNull();
  });

  test("a language answer from before an engine pick does not undo it", async () => {
    const { result } = await loaded();
    const language = deferred<EngineChoices>();
    ipc.setSpokenLanguage.mockReturnValueOnce(language.promise);
    // Rust writes the language first, so the engine answer carries it.
    ipc.setTranscription.mockResolvedValueOnce({ ...base, model: "small", spokenLanguage: "de" });

    act(() => void result.current.pickLanguage("de"));
    await act(async () => result.current.pickModel("small"));
    await act(async () => language.resolve({ ...base, spokenLanguage: "de" }));

    expect(result.current.choices?.model).toBe("small");
    expect(result.current.choices?.spokenLanguage).toBe("de");
    expect(ipc.setTranscription).toHaveBeenCalledWith(base.engine, "small");
  });

  test("the newest save that fails puts back what was shown before it", async () => {
    const { result } = await loaded();
    ipc.setTranscription.mockRejectedValueOnce({ domain: "app", kind: "io", message: "disk" });

    await act(async () => result.current.pickModel("small"));

    expect(result.current.choices?.model).toBe(base.model);
    expect(result.current.saveError?.message).toBe("disk");
  });
});
