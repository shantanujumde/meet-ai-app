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
 *
 * Clears `sessionStorage` too: the recording store remembers which error the
 * user dismissed there (TUR-127), and a dismissal left over from one test
 * would hide the banner another test is looking for.
 */
export async function resetStores(): Promise<void> {
  sessionStorage.clear();
  const [
    { useAppStore },
    { useRecordingStore },
    { useTranscriptStore },
    { useAppearanceStore },
    { useSessionStore },
  ] = await Promise.all([
    import("@/state/app"),
    import("@/state/recording"),
    import("@/state/transcript"),
    import("@/state/appearance"),
    import("@/state/session"),
  ]);
  useAppStore.setState(useAppStore.getInitialState(), true);
  useAppearanceStore.setState(useAppearanceStore.getInitialState(), true);
  useRecordingStore.setState(useRecordingStore.getInitialState(), true);
  useTranscriptStore.setState(useTranscriptStore.getInitialState(), true);
  useSessionStore.setState(useSessionStore.getInitialState(), true);
}

beforeEach(async () => {
  resetIpcMock();
  await resetStores();
});
