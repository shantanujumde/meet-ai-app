/**
 * Model download state that outlives the screen showing it (TUR-159).
 *
 * A download runs in Rust for minutes, but the speech card that started it is
 * gone as soon as the user leaves Settings or the onboarding speech step. Kept
 * in component state, coming back mid-download showed an enabled "Download"
 * with no bar, and a click started a second download of the same model. The
 * state lives here instead, at module level, and the pending download keeps
 * writing to it whether or not anything is on screen.
 */

import type { ModelState } from "./ModelRow";

export type DownloadStates = Record<string, ModelState>;

let states: DownloadStates = {};
const listeners = new Set<() => void>();

/** The current states, keyed by model id. Same object until something changes. */
export function downloadStates(): DownloadStates {
  return states;
}

/** Replace the states with `change(previous)` and tell every subscriber. */
export function updateDownloadStates(change: (previous: DownloadStates) => DownloadStates): void {
  states = change(states);
  for (const listener of [...listeners]) listener();
}

/** For `useSyncExternalStore`. */
export function subscribeDownloadStates(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * A fresh screen keeps only what is still running: an error or a finished
 * download from the last visit is not news, a download in flight is.
 */
export function keepRunningDownloads(): void {
  updateDownloadStates((previous) => {
    const running = Object.entries(previous).filter(([, state]) => state.busy);
    if (running.length === Object.keys(previous).length) return previous;
    return Object.fromEntries(running);
  });
}
