/**
 * One Google or Microsoft sign-in, as the calendar screens run it (TUR-49):
 * the browser opens, the app waits, and the provider is added to the
 * calendars meet-ai reads. Shared by the Settings rows and
 * {@link SignInButtons}, so both say the same thing while waiting and when
 * it fails.
 *
 * TUR-174: the wait can be cancelled. The browser cannot tell the app its
 * tab was closed, so without Cancel the screen would sit busy for minutes.
 * A sign-in the user cancelled ends quietly, with no error line.
 */

import { useRef, useState } from "react";
import {
  type CalendarAccount,
  calendarCancelSignIn,
  calendarConnect,
  type SignInProvider,
} from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { signInError } from "./copy";

export function useCalendarSignIn(onConnected: (account: CalendarAccount) => void) {
  /** The provider whose sign-in is waiting on the browser, if any. */
  const [connecting, setConnecting] = useState<SignInProvider | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  /** The user pressed Cancel on the sign-in now waiting. */
  const cancelled = useRef(false);

  async function connect(provider: SignInProvider) {
    setConnecting(provider);
    setError(null);
    cancelled.current = false;
    try {
      onConnected(await calendarConnect(provider));
    } catch (thrown) {
      const failure = toUiError(thrown);
      if (!(cancelled.current && isCancelled(failure))) {
        setError(signInError(provider, failure));
      }
    } finally {
      setConnecting(null);
    }
  }

  async function cancel() {
    if (connecting === null) return;
    cancelled.current = true;
    try {
      // `false`: nothing was waiting yet (or any more); the sign-in goes on.
      if (!(await calendarCancelSignIn(connecting))) cancelled.current = false;
    } catch (thrown) {
      cancelled.current = false;
      setError(toUiError(thrown));
    }
  }

  return { connecting, error, setError, connect, cancel };
}

function isCancelled(error: UiError): boolean {
  return error.domain === "app" && error.kind === "calendar-sign-in-cancelled";
}
