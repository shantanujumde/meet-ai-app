/**
 * "Sign in with Google" and "Sign in with Microsoft", side by side (TUR-49):
 * the Today pane's empty state and onboarding's calendar step. Each opens
 * the browser, waits for the sign-in, and adds that calendar to the ones
 * meet-ai reads. Settings has its own per-account rows instead.
 */

import { useState } from "react";
import {
  type CalendarAccount,
  calendarConnect,
  SIGN_IN_PROVIDERS,
  type SignInProvider,
} from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { Button, ButtonRow } from "../primitives";
import { InlineError } from "../states";
import { signInError, signInLabel } from "./copy";

export const WAITING_FOR_BROWSER = "Waiting for the browser…";

export function SignInButtons({
  onConnected,
}: {
  /** The sign-in finished, and the calendar is now read. */
  onConnected: (account: CalendarAccount) => void;
}) {
  const [busy, setBusy] = useState<SignInProvider | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  async function connect(provider: SignInProvider) {
    setBusy(provider);
    setError(null);
    try {
      onConnected(await calendarConnect(provider));
    } catch (thrown) {
      setError(signInError(provider, toUiError(thrown)));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <ButtonRow>
        {SIGN_IN_PROVIDERS.map((provider) => (
          <Button key={provider} disabled={busy !== null} onClick={() => void connect(provider)}>
            {busy === provider ? WAITING_FOR_BROWSER : signInLabel(provider)}
          </Button>
        ))}
      </ButtonRow>
      {error ? <InlineError error={error} /> : null}
    </>
  );
}
