/**
 * "Read a value from Rust on mount, then let the screen change it" (TUR-178):
 * the pattern every Settings card and a few screens wrote out by hand, each
 * with its own `let live` guard.
 *
 * {@link useIpcValue} reads once per `load` (pass a function defined outside
 * the component, or a memoised one) and drops an answer that lands after
 * unmount or after `load` changed. {@link useSavedSetting} adds the save: busy
 * while it runs, the saved value shown after, and a failed save keeps the old
 * value and shows why.
 */

import { type Dispatch, type SetStateAction, useCallback, useEffect, useState } from "react";
import { toUiError, type UiError } from "@/ipc/types";

export type IpcValue<T> = {
  /** Null until `load` answers. */
  value: T | null;
  setValue: Dispatch<SetStateAction<T | null>>;
  /** Why `load` failed; also for the screen's own errors on the same spot. */
  error: UiError | null;
  setError: Dispatch<SetStateAction<UiError | null>>;
};

export function useIpcValue<T>(load: () => Promise<T>): IpcValue<T> {
  const [value, setValue] = useState<T | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    let live = true;
    load().then(
      (answer) => {
        if (live) setValue(() => answer);
      },
      (thrown: unknown) => {
        if (live) setError(toUiError(thrown));
      },
    );
    return () => {
      live = false;
    };
  }, [load]);

  return { value, setValue, error, setError };
}

export type SavedSetting<T> = IpcValue<T> & {
  /** A save is in flight. */
  busy: boolean;
  /** Save `next` and show what `save` answers was saved. */
  change: (next: T) => Promise<void>;
};

export function useSavedSetting<T>(
  load: () => Promise<T>,
  save: (next: T) => Promise<T>,
): SavedSetting<T> {
  const loaded = useIpcValue(load);
  const { setValue, setError } = loaded;
  const [busy, setBusy] = useState(false);

  const change = useCallback(
    async (next: T) => {
      setBusy(true);
      setError(null);
      try {
        const saved = await save(next);
        setValue(() => saved);
      } catch (thrown) {
        setError(toUiError(thrown));
      } finally {
        setBusy(false);
      }
    },
    [save, setValue, setError],
  );

  return { ...loaded, busy, change };
}
