import "@testing-library/jest-dom/vitest";
import { beforeEach } from "vitest";
import { resetIpcMock } from "./ipcMock";

/**
 * Put every zustand store back to the state it was created with.
 *
 * The stores are module state, so one test's recording, meeting list or live
 * transcript would otherwise leak into the next. Imported lazily, inside the
 * reset, so this setup file never loads a store — and through it
 * `@/ipc/client` — before a test file's `vi.mock` of the client is in place.
 */
export async function resetStores(): Promise<void> {
  const [{ useAppStore }, { useRecordingStore }, { useTranscriptStore }] = await Promise.all([
    import("@/state/app"),
    import("@/state/recording"),
    import("@/state/transcript"),
  ]);
  useAppStore.setState(useAppStore.getInitialState(), true);
  useRecordingStore.setState(useRecordingStore.getInitialState(), true);
  useTranscriptStore.setState(useTranscriptStore.getInitialState(), true);
}

beforeEach(async () => {
  resetIpcMock();
  await resetStores();
});
