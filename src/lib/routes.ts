/**
 * Every path the app navigates to, in one place.
 *
 * The route table in App.tsx and the screens that link into it used to spell
 * these out by hand — `/meetings/${…}` three times, `/onboarding` in four
 * files — so renaming one meant finding all of them. Build a path here and the
 * two cannot drift.
 */

import type { meet_ai_lib_lifecycle_NavigateTo as NavigateTo } from "@/ipc/bindings";

/** The meeting list, and where a four-route app lands when a hash matches nothing. */
export const MEETINGS = "/meetings";

export const SETTINGS = "/settings";

export const TICKETS = "/tickets";

/** The pre-meeting brief (TUR-32). Build links with {@link briefPath}. */
export const BRIEF = "/brief";

/** The setup wizard. Its steps live under it — see {@link onboardingStepPath}. */
export const ONBOARDING = "/onboarding";

/** The wizard's steps, in order. The route's `:step` is one of these. */
export const ONBOARDING_STEPS = [
  "welcome",
  "permission",
  "speech",
  "folder",
  // TUR-49: the optional calendar sign-in, only off macOS (`onboardingSteps`).
  "calendar",
  "agent",
] as const;
export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

/**
 * The wizard's steps on this OS. The calendar step is only for Windows and
 * Linux: a Mac reads the Calendar app with no setup (TUR-49).
 */
export function onboardingSteps(calendarAppAvailable: boolean): readonly OnboardingStep[] {
  return calendarAppAvailable
    ? ONBOARDING_STEPS.filter((step) => step !== "calendar")
    : ONBOARDING_STEPS;
}

/** Whether a `:step` param names a real step, rather than a typo or a stale URL. */
export function isOnboardingStep(value: string | undefined): value is OnboardingStep {
  return ONBOARDING_STEPS.includes(value as OnboardingStep);
}

/**
 * The permission step, reached after setup only as a deliberate trip (TUR-127).
 * Navigate there with `openPermissionScreen`, never to this path bare.
 */
export const PERMISSION_ROUTE = onboardingStepPath("permission");

/**
 * One meeting's review screen.
 *
 * Encoded because a meeting id is a folder name, and folder names can hold
 * spaces or a `#` that would otherwise end the hash route.
 */
export function meetingPath(id: string): string {
  return `${MEETINGS}/${encodeURIComponent(id)}`;
}

/** One step of the setup wizard. */
export function onboardingStepPath(step: OnboardingStep): string {
  return `${ONBOARDING}/${step}`;
}

/** Whether a pathname is somewhere inside the setup wizard. */
export function isOnboardingPath(pathname: string): boolean {
  return pathname.startsWith(ONBOARDING);
}

/**
 * The pre-meeting brief for the meeting called `title`: `/brief?title=<title>`,
 * the link the reminder notification (TUR-30) and the Today pane open.
 */
export function briefPath(title: string): string {
  return `${BRIEF}?${new URLSearchParams({ title }).toString()}`;
}

/**
 * Where a navigation Rust asked for (TUR-77: the menu bar's "Open brief" and
 * "Calendar not connected") lands.
 */
export function navigationPath(to: NavigateTo): string {
  return to.to === "brief" ? briefPath(to.title) : SETTINGS;
}
