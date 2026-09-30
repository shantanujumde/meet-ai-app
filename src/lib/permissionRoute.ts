/**
 * The way back to the permission screen once onboarding is finished.
 *
 * Bootstrap (App.tsx) sends a finished user off any `/onboarding` URL, so a
 * leftover setup page cannot trap them there (TUR-79). A button that sends
 * someone to fix their audio permission is a deliberate trip, not a leftover,
 * so it carries this state and Bootstrap lets it through (TUR-127). Every
 * such button goes through here; one that navigates to the path bare is
 * bounced straight back to the list.
 */

import type { NavigateFunction } from "react-router";
import { PERMISSION_ROUTE } from "./routes";

type RevisitState = { revisit: true };

export function openPermissionScreen(navigate: NavigateFunction): void {
  const state: RevisitState = { revisit: true };
  void navigate(PERMISSION_ROUTE, { state });
}

/** Whether this navigation was a deliberate trip from {@link openPermissionScreen}. */
export function isRevisit(state: unknown): boolean {
  return (state as Partial<RevisitState> | null)?.revisit === true;
}
