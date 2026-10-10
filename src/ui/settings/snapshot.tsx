/**
 * One read of the saved settings per Settings visit (TUR-171).
 *
 * {@link SettingsSnapshotProvider} asks `settings_snapshot` once, as the
 * screen first renders, and every card inside it takes its saved value from
 * that one answer instead of asking its own command. A card outside it (the
 * agent card in onboarding, a card's own test) asks its own command as
 * before, and so does a card whose snapshot could not be read.
 *
 * Each visit reads a fresh snapshot, so a value changed by hand in
 * `config.jsonc` shows the next time Settings opens.
 */

import { createContext, type ReactNode, useCallback, useContext, useState } from "react";
import { type SettingsSnapshot, settingsSnapshot } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { rememberSnapshot } from "@/state/session";

type Snapshot = Promise<SettingsSnapshot | null>;

const SnapshotContext = createContext<Snapshot | null>(null);

export function SettingsSnapshotProvider({ children }: { children: ReactNode }) {
  // Asked during the first render, before any card's effect asks for it.
  const [snapshot] = useState<Snapshot>(() => {
    const read = settingsSnapshot();
    // The meeting view keeps a few of these for the session; a fresh read
    // brings them up to date.
    read.then((answer) => (answer ? rememberSnapshot(answer) : undefined)).catch(() => {});
    return read;
  });
  return <SnapshotContext.Provider value={snapshot}>{children}</SnapshotContext.Provider>;
}

/** This visit's snapshot, or null outside Settings. */
export function useSettingsSnapshot(): Snapshot | null {
  return useContext(SnapshotContext);
}

/**
 * `pick` from `snapshot`, or `fallback()` when there is none or it could not
 * be read. A `pick` that throws (a section that did not read) rejects with
 * what it threw, as the card's own command would have.
 */
export function fromSnapshot<T>(
  snapshot: Snapshot | null,
  pick: (snapshot: SettingsSnapshot) => T,
  fallback: () => Promise<T>,
): Promise<T> {
  if (!snapshot) return fallback();
  return snapshot.then(
    (answer) => (answer ? pick(answer) : fallback()),
    () => fallback(),
  );
}

/**
 * A card's load function: its value from this visit's snapshot, else its own
 * command. The same function for the whole visit, so a card that reloads on
 * a new `load` reads once. Pass module-level `pick` and `fallback`.
 */
export function useSnapshotLoad<T>(
  pick: (snapshot: SettingsSnapshot) => T,
  fallback: () => Promise<T>,
): () => Promise<T> {
  const snapshot = useSettingsSnapshot();
  return useCallback(() => fromSnapshot(snapshot, pick, fallback), [snapshot, pick, fallback]);
}

/** `value`, or a throw of `error` when the section did not read. */
export function readOrThrow<T>(value: T | null, error: UiError | null): T {
  if (value !== null) return value;
  throw (
    error ?? { domain: "app", kind: "invalid-config", message: "config.jsonc could not be read" }
  );
}
