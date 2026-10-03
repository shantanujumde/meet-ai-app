/**
 * One Google or Microsoft sign-in, as the calendar screens run it (TUR-49):
 * the browser opens, the app waits, and the provider is added to the
 * calendars meet-ai reads. Shared by the Settings rows and
 * {@link SignInButtons}, so both say the same thing while waiting and when
 * it fails.
 */

import { useState } from "react";
import { type CalendarAccount, calendarConnect, type SignInProvider } from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { signInError } from "./copy";

export function useCalendarSignIn(onConnected: (account: CalendarAccount) => void) {
  /** The provider whose sign-in is waiting on the browser, if any. */
  const [connecting, setConnecting] = useState<SignInProvider | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  async function connect(provider: SignInProvider) {
    setConnecting(provider);
    setError(null);
    try {
      onConnected(await calendarConnect(provider));
    } catch (thrown) {
      setError(signInError(provider, toUiError(thrown)));
    } finally {
      setConnecting(null);
    }
  }

  return { connecting, error, setError, connect };
}
