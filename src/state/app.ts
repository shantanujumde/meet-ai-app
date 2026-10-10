/**
 * Everything the shell needs to know that is not the recording state:
 * the meeting list, the audio permission, and whether onboarding is done.
 *
 * These live in one store because they change together. Finishing onboarding
 * changes which route the app lands on; stopping a recording adds a meeting to
 * the list; fixing permission in System Settings changes the banner above every
 * screen. Three stores would mean three chances for them to disagree.
 *
 * Each slice carries its own `loading` and `error` rather than sharing one
 * flag, so a failed permission check cannot blank out a meeting list that
 * loaded perfectly well.
 */

import { create } from "zustand";
import {
  completeOnboarding,
  listMeetings,
  measurePermission,
  onboardingState,
  onPermissionStatus,
  permissionQuick,
  resetOnboarding,
} from "@/ipc/client";
import { ROOT_POINTER_UNREADABLE } from "@/ipc/errors";
import type { MeetingList, OnboardingState, PermissionStatus, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

type AppStore = {
  meetings: MeetingList | null;
  meetingsError: UiError | null;
  meetingsLoading: boolean;
  loadMeetings: (options?: { silent?: boolean }) => Promise<void>;

  permission: PermissionStatus | null;
  permissionLoading: boolean;
  /**
   * `silent` skips the audible positive-control check (TUR-24's chime) for the
   * microphone's stored decision alone — what the app runs on launch.
   */
  loadPermission: (options?: { silent?: boolean }) => Promise<void>;

  onboarding: OnboardingState | null;
  onboardingLoading: boolean;
  /**
   * Why "Show setup again" did not work. Its own field, not `meetingsError`:
   * a setup flag that failed to reset says nothing about the meeting list, and
   * putting it there replaced a perfectly good list with an error screen.
   */
  onboardingError: UiError | null;
  loadOnboarding: () => Promise<void>;
  finishOnboarding: () => Promise<void>;
  restartOnboarding: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set, get) => ({
  meetings: null,
  meetingsError: null,
  // Starts true: the very first render is a load, not an empty list. Showing
  // "No meetings yet" for one frame before the real ones arrive is a flicker
  // that reads as data loss.
  meetingsLoading: true,

  async loadMeetings(options) {
    if (options?.silent) {
      // A background refresh: no loading flicker, and a failure keeps the
      // list already on screen rather than replacing it with an error.
      try {
        set({ meetings: await listMeetings(), meetingsError: null });
      } catch {
        // Keep the old list.
      }
      return;
    }

    set({ meetingsLoading: true });
    try {
      set({ meetings: await listMeetings(), meetingsError: null });
    } catch (thrown) {
      set({ meetingsError: toUiError(thrown) });
    } finally {
      set({ meetingsLoading: false });
    }
  },

  permission: null,
  permissionLoading: true,

  async loadPermission(options) {
    if (options?.silent) {
      // `permissionLoading` is left alone: it drives onboarding's "Checking…"
      // spinner, which only the full check should switch off.
      try {
        const status = await permissionQuick();
        // Knows less than a measured answer, so never replaces one — a
        // first launch runs onboarding's full check alongside this.
        if (!get().permission?.measured) set({ permission: status });
      } catch {
        // Nothing learned. Record still runs the full check before starting.
      }
      return;
    }

    set({ permissionLoading: true });
    try {
      set({ permission: await measurePermission() });
    } catch (thrown) {
      // A permission check that itself fails is still an unknown answer, not a
      // denial. Reporting it as denied would send the user to System Settings
      // to fix something that may not be broken.
      set({
        permission: {
          state: "unknown",
          measured: false,
          detail: toUiError(thrown).message,
          denied: [],
        },
      });
    } finally {
      set({ permissionLoading: false });
    }
  },

  onboarding: null,
  onboardingLoading: true,
  onboardingError: null,

  async loadOnboarding() {
    set({ onboardingLoading: true });
    try {
      set({ onboarding: await onboardingState() });
    } catch (thrown) {
      // TUR-149: with the meetings-folder pointer damaged, whether setup was
      // done is unknown, not "no": the flag lives in the folder the pointer
      // names. Leaving `onboarding` null keeps the user out of the wizard,
      // and the meetings screen shows the error and how to fix it.
      const error = toUiError(thrown);
      if (error.domain === "app" && error.kind === ROOT_POINTER_UNREADABLE) {
        set({ onboarding: null });
        return;
      }
      // Unreadable means not onboarded. Seeing the steps twice is a small
      // annoyance; never seeing them means recording silence.
      set({ onboarding: { completedAt: null } });
    } finally {
      set({ onboardingLoading: false });
    }
  },

  async finishOnboarding() {
    try {
      set({ onboarding: await completeOnboarding() });
    } catch {
      // The flag did not persist, so onboarding will reappear next launch.
      // Let the user through now regardless — blocking them out of the app
      // over a bookkeeping file would be absurd.
      set({ onboarding: { completedAt: new Date().toISOString() } });
    }
  },

  async restartOnboarding() {
    set({ onboardingError: null });
    try {
      set({ onboarding: await resetOnboarding() });
    } catch (thrown) {
      set({ onboardingError: toUiError(thrown) });
    }
  },
}));

/**
 * Follow the permission check Rust runs at every recording start.
 *
 * That check is the only one that hears system audio after launch (launch is
 * silent, SPEC A7), so without this a refused Record left the button enabled
 * and "Fix this" hidden, and a grant restored in Settings stayed "denied"
 * until a relaunch. Returns the teardown, like `watchRecordingState`.
 */
export function watchPermissionStatus(): () => void {
  return onPermissionStatus((status) => {
    useAppStore.setState({ permission: status });
  });
}
