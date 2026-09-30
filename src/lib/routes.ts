/**
 * Every path the app navigates to, in one place.
 *
 * The route table in App.tsx and the screens that link into it used to spell
 * these out by hand — `/meetings/${…}` three times, `/onboarding` in four
 * files — so renaming one meant finding all of them. Build a path here and the
 * two cannot drift.
 */

/** The meeting list, and where a four-route app lands when a hash matches nothing. */
export const MEETINGS = "/meetings";

export const SETTINGS = "/settings";

/** The setup wizard. Its steps live under it — see {@link onboardingStepPath}. */
export const ONBOARDING = "/onboarding";

/** The wizard's steps, in order. The route's `:step` is one of these. */
export const ONBOARDING_STEPS = ["welcome", "permission", "speech", "folder"] as const;
export type OnboardingStep = (typeof ONBOARDING_STEPS)[number];

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
