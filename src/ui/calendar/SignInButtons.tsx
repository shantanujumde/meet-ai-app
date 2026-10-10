/**
 * "Sign in with Google" and "Sign in with Microsoft", side by side (TUR-49):
 * the Today pane's empty state and onboarding's calendar step. Each opens
 * the browser, waits for the sign-in, and adds that calendar to the ones
 * meet-ai reads. While it waits, Cancel stops waiting (TUR-174). Settings has its own per-account rows instead.
 */

import { LogIn, X } from "lucide-react";
import { type CalendarAccount, SIGN_IN_PROVIDERS } from "@/ipc/client";
import { Button, ButtonRow } from "../primitives";
import { InlineError } from "../states";
import { signInLabel } from "./copy";
import { useCalendarSignIn } from "./useCalendarSignIn";

export const WAITING_FOR_BROWSER = "Waiting for the browser…";

/** Stops waiting for the browser (TUR-174). */
export const CANCEL_SIGN_IN = "Cancel";

export function SignInButtons({
  onConnected,
}: {
  /** The sign-in finished, and the calendar is now read. */
  onConnected: (account: CalendarAccount) => void;
}) {
  const { connecting, error, connect, cancel } = useCalendarSignIn(onConnected);

  return (
    <>
      <ButtonRow>
        {SIGN_IN_PROVIDERS.map((provider) => (
          <Button
            key={provider}
            icon={LogIn}
            disabled={connecting !== null}
            onClick={() => void connect(provider)}
          >
            {connecting === provider ? WAITING_FOR_BROWSER : signInLabel(provider)}
          </Button>
        ))}
        {connecting !== null ? (
          <Button icon={X} tone="quiet" onClick={() => void cancel()}>
            {CANCEL_SIGN_IN}
          </Button>
        ) : null}
      </ButtonRow>
      {error ? <InlineError error={error} /> : null}
    </>
  );
}
