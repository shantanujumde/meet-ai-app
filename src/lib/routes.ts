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

/**
 * The permission step, reached after setup only as a deliberate trip (TUR-127).
 * Navigate there with `openPermissionScreen`, never to this path bare.
 */
export const PERMISSION_ROUTE = `${ONBOARDING}/permission`;

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
export function onboardingStepPath(step: string): string {
  return `${ONBOARDING}/${step}`;
}

/** Whether a pathname is somewhere inside the setup wizard. */
export function isOnboardingPath(pathname: string): boolean {
  return pathname.startsWith(ONBOARDING);
}
