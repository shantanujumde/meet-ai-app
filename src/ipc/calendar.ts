/**
 * Where meetings come from (TUR-49): the Calendar app on macOS, and the
 * Google and Microsoft sign-ins on every OS.
 *
 * Re-exported from `./client`; import from there.
 */

import {
  type meet_ai_lib_calendar_signin_CalendarAccount as CalendarAccount,
  type meet_ai_lib_calendar_sources_CalendarSources as CalendarSources,
  commands,
  type meet_ai_lib_calendar_signin_SignInProvider as SignInProvider,
} from "./bindings";
import { call, hasBackend } from "./client";

export type { CalendarAccount, CalendarSources, SignInProvider };

/** Both sign-ins, in the order the screens list them. */
export const SIGN_IN_PROVIDERS: readonly SignInProvider[] = ["google", "microsoft"];

/** What a fresh Mac has: the Calendar app on, no sign-in set up. */
export const DEFAULT_CALENDAR_SOURCES: CalendarSources = {
  calendarAppAvailable: true,
  calendarApp: true,
  configured: [],
  connected: [],
};

/** Which sources this OS offers and which are set up. A config read only. */
export async function calendarSources(): Promise<CalendarSources> {
  if (!hasBackend()) return DEFAULT_CALENDAR_SOURCES;
  return call(() => commands.calendarSources());
}

/**
 * Both sign-ins' state: signed in (with the account), expired, or signed
 * out. The first call after a launch may wait on the network.
 */
export async function calendarAccounts(): Promise<CalendarAccount[]> {
  if (!hasBackend()) return [];
  return call(() => commands.calendarAccounts());
}

/** Read the Calendar app or stop reading it (macOS). Resolves to what was saved. */
export function setCalendarApp(on: boolean): Promise<CalendarSources> {
  return call(() => commands.setCalendarApp(on));
}

/**
 * Sign in in the browser, then read that calendar. Rejects with kind
 * `calendar-not-configured` when `config.jsonc` has no client id, and
 * `calendar-sign-in-cancelled` when the browser never came back.
 */
export function calendarConnect(provider: SignInProvider): Promise<CalendarAccount> {
  return call(() => commands.calendarConnect(provider));
}

/** Sign out and stop reading that calendar. */
export function calendarDisconnect(provider: SignInProvider): Promise<CalendarSources> {
  return call(() => commands.calendarDisconnect(provider));
}
