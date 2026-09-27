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
  onboardingState,
  permissionStatus,
  resetOnboarding,
} from "@/ipc/client";
import type { MeetingList, OnboardingState, PermissionStatus, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

type AppStore = {
  meetings: MeetingList | null;
  meetingsError: UiError | null;
  meetingsLoading: boolean;
  loadMeetings: () => Promise<void>;

  permission: PermissionStatus | null;
  permissionLoading: boolean;
  loadPermission: () => Promise<void>;

  onboarding: OnboardingState | null;
  onboardingLoading: boolean;
  loadOnboarding: () => Promise<void>;
  finishOnboarding: () => Promise<void>;
  restartOnboarding: () => Promise<void>;
};

export const useAppStore = create<AppStore>((set) => ({
  meetings: null,
  meetingsError: null,
  // Starts true: the very first render is a load, not an empty list. Showing
  // "No meetings yet" for one frame before the real ones arrive is a flicker
  // that reads as data loss.
  meetingsLoading: true,

  async loadMeetings() {
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

  async loadPermission() {
    set({ permissionLoading: true });
    try {
      set({ permission: await permissionStatus() });
    } catch (thrown) {
      // A permission check that itself fails is still an unknown answer, not a
      // denial. Reporting it as denied would send the user to System Settings
      // to fix something that may not be broken.
      set({
        permission: {
          state: "unknown",
          measured: false,
          detail: toUiError(thrown).message,
        },
      });
    } finally {
      set({ permissionLoading: false });
    }
  },

  onboarding: null,
  onboardingLoading: true,

  async loadOnboarding() {
    set({ onboardingLoading: true });
    try {
      set({ onboarding: await onboardingState() });
    } catch {
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
    try {
      set({ onboarding: await resetOnboarding() });
    } catch (thrown) {
      set({ meetingsError: toUiError(thrown) });
    }
  },
}));
