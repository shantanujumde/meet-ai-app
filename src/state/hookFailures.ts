/**
 * The user hooks that failed, per meeting, since the app started (TUR-63,
 * TUR-170).
 *
 * Watched from the app root rather than from the meeting view: a hook that
 * fails after Stop while the user is on another screen is still shown when
 * they open that meeting, and the note does not disappear after one look.
 * Rust keeps no list of past failures, so a restart starts empty; the full
 * output is in the app log.
 */

import { create } from "zustand";
import { type HookFailed, onHookFailed } from "@/ipc/client";

type HookFailuresStore = {
  /** Each meeting's failed hooks, the latest failure per hook. */
  byMeeting: Record<string, HookFailed[]>;
  /** Record a failure. Not for components to call. */
  add: (failed: HookFailed) => void;
};

export const useHookFailures = create<HookFailuresStore>((set) => ({
  byMeeting: {},
  add(failed) {
    set(({ byMeeting }) => {
      const shown = byMeeting[failed.meetingId] ?? [];
      return {
        byMeeting: {
          ...byMeeting,
          [failed.meetingId]: [...shown.filter((f) => f.hook !== failed.hook), failed],
        },
      };
    });
  },
}));

const NONE: HookFailed[] = [];

/** One meeting's failed hooks, oldest first. */
export function useMeetingHookFailures(meetingId: string): HookFailed[] {
  return useHookFailures((state) => state.byMeeting[meetingId] ?? NONE);
}

/** Start keeping hook failures. Returns the teardown, for StrictMode. */
export function watchHookFailures(): () => void {
  return onHookFailed((failed) => useHookFailures.getState().add(failed));
}
