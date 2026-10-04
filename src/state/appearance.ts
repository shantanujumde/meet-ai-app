/**
 * The window's appearance (TUR-102): Light / Dark / System and the glass
 * switch, as saved in `config.jsonc`, kept on `<html>` while the app runs.
 *
 * Its own store, not a slice of `app.ts`: the record prompt's popup window
 * (TUR-59) needs it too, and has none of the rest.
 */

import { create } from "zustand";
import { type Appearance, appearanceSettings, setAppearance } from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { applyAppearance, watchSystemTheme } from "@/lib/appearance";
import { DEFAULT_APPEARANCE } from "@/lib/constants";

type AppearanceStore = {
  appearance: Appearance;
  /** False until the saved value has been read; the controls wait for it. */
  loaded: boolean;
  saving: boolean;
  error: UiError | null;
  load: () => Promise<void>;
  /** Save a change; `<html>` changes at once and goes back if the save fails. */
  save: (change: Partial<Appearance>) => Promise<void>;
};

export const useAppearanceStore = create<AppearanceStore>((set, get) => ({
  appearance: DEFAULT_APPEARANCE,
  loaded: false,
  saving: false,
  error: null,

  load: async () => {
    try {
      const appearance = await appearanceSettings();
      set({ appearance, loaded: true, error: null });
    } catch (thrown) {
      // A config problem costs the look, never the app: keep the defaults.
      set({ loaded: true, error: toUiError(thrown) });
    }
  },

  save: async (change) => {
    const before = get().appearance;
    const next = { ...before, ...change };
    set({ appearance: next, saving: true, error: null });
    try {
      set({ appearance: await setAppearance(next), saving: false });
    } catch (thrown) {
      set({ appearance: before, saving: false, error: toUiError(thrown) });
    }
  },
}));

/**
 * Apply the appearance now and on every change, from the store and from the
 * OS switching light and dark under "System". Call once, before the first
 * render, so the window never paints in the wrong colours. Returns the
 * unsubscribe.
 */
export function watchAppearance(root: HTMLElement = document.documentElement): () => void {
  const apply = () => applyAppearance(useAppearanceStore.getState().appearance, root);
  apply();
  const stopStore = useAppearanceStore.subscribe((state, previous) => {
    if (state.appearance !== previous.appearance) apply();
  });
  const stopSystem = watchSystemTheme(apply);
  void useAppearanceStore.getState().load();
  return () => {
    stopStore();
    stopSystem();
  };
}
