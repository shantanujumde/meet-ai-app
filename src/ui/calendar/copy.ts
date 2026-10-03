/**
 * The words the calendar screens share (TUR-49): provider names, and what to
 * say when a sign-in has no client id or did not finish.
 */

import type { SignInProvider } from "@/ipc/client";
import type { UiError } from "@/ipc/types";

export const PROVIDER_NAME: Record<SignInProvider, string> = {
  google: "Google",
  microsoft: "Microsoft",
};

/** The Today pane and onboarding, with no calendar to read. */
export const SIGN_IN_TO_SEE_TODAY = "Sign in with Google or Microsoft to see today's meetings";

export function signInLabel(provider: SignInProvider): string {
  return `Sign in with ${PROVIDER_NAME[provider]}`;
}

/** No `client_id` in `config.jsonc`: the key to add, instead of a button that cannot work. */
export function notConfiguredCopy(provider: SignInProvider): string {
  return `Add calendar.${provider}.client_id to config.jsonc (see SETUP.md)`;
}

export function isNotConfigured(error: UiError): boolean {
  return error.domain === "app" && error.kind === "calendar-not-configured";
}

/**
 * A failed sign-in as one line. A missing client id names the key; anything
 * else is Rust's own sentence, which is already worded for the user.
 */
export function signInError(provider: SignInProvider, error: UiError): UiError {
  return isNotConfigured(error) ? { ...error, message: notConfiguredCopy(provider) } : error;
}
