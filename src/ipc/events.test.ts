import { afterEach, beforeEach, expect, test, vi } from "vitest";

const listen = vi.hoisted(() => vi.fn());
const warn = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-log", () => ({ warn }));

import { RECORDING_STATE_EVENT } from "./bindings";
import { subscribe } from "./events";

/** The webview's bridge, so `hasBackend()` says there is a Rust side. */
beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  listen.mockReset();
  warn.mockReset();
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

test("a failed listen is logged, not swallowed (TUR-173)", async () => {
  listen.mockRejectedValue(new Error("event.listen not allowed"));
  warn.mockResolvedValue(undefined);

  subscribe(RECORDING_STATE_EVENT, () => {});

  await vi.waitFor(() => expect(warn).toHaveBeenCalledTimes(1));
  expect(warn.mock.calls[0]?.[0]).toContain(RECORDING_STATE_EVENT);
  expect(warn.mock.calls[0]?.[0]).toContain("event.listen not allowed");
});

test("when the log cannot be written either, the console says it", async () => {
  listen.mockRejectedValue(new Error("no capability"));
  warn.mockRejectedValue(new Error("log not allowed"));
  const console = vi.spyOn(globalThis.console, "warn").mockImplementation(() => {});

  subscribe(RECORDING_STATE_EVENT, () => {});

  await vi.waitFor(() => expect(console).toHaveBeenCalledTimes(1));
  expect(String(console.mock.calls[0]?.[0])).toContain("no capability");
  console.mockRestore();
});

test("the payload reaches the handler", async () => {
  let deliver: ((event: { payload: unknown }) => void) | undefined;
  listen.mockImplementation(async (_event: string, onEvent: typeof deliver) => {
    deliver = onEvent;
    return () => {};
  });
  const handler = vi.fn();

  subscribe(RECORDING_STATE_EVENT, handler);
  await vi.waitFor(() => expect(deliver).toBeDefined());
  deliver?.({ payload: { phase: "idle" } });

  expect(listen).toHaveBeenCalledWith(RECORDING_STATE_EVENT, expect.any(Function));
  expect(handler).toHaveBeenCalledWith({ phase: "idle" });
});

test("unsubscribing before listen resolves still removes the listener", async () => {
  const stop = vi.fn();
  listen.mockResolvedValue(stop);

  subscribe(RECORDING_STATE_EVENT, () => {})();

  await vi.waitFor(() => expect(stop).toHaveBeenCalledTimes(1));
  expect(warn).not.toHaveBeenCalled();
});
