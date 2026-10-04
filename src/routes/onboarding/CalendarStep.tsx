/**
 * Onboarding, the optional calendar step (TUR-49): only on Windows and
 * Linux, which have no Calendar app, so a sign-in is their only way to see
 * meetings. On macOS the Calendar app is read with no setup, and this step
 * is left out of the wizard (`onboardingSteps`).
 *
 * Optional: **Skip** is always there, and Settings → Calendars does the
 * same thing later.
 */

import { useState } from "react";
import type { CalendarAccount } from "@/ipc/client";
import { PROVIDER_NAME, SIGN_IN_TO_SEE_TODAY } from "@/ui/calendar/copy";
import { SignInButtons } from "@/ui/calendar/SignInButtons";
import { Button, ButtonRow, Prose } from "@/ui/primitives";

export function CalendarStep({ onNext }: { onNext: () => void }) {
  const [signedIn, setSignedIn] = useState<CalendarAccount | null>(null);

  return (
    <>
      <header className="page__header">
        <h1 className="page__title">See today's meetings</h1>
      </header>
      <Prose>
        {SIGN_IN_TO_SEE_TODAY}. meet-ai uses your calendar to name recordings, remind you before a
        meeting starts, and show what happened last time. It only reads, and never changes your
        calendar. This is optional, and Settings → Calendars can do it later.
      </Prose>

      {signedIn ? (
        <p className="text-callout text-success" role="status">
          Signed in to {PROVIDER_NAME[signedIn.provider]}
          {signedIn.account ? ` as ${signedIn.account}` : ""}.
        </p>
      ) : (
        <SignInButtons onConnected={setSignedIn} />
      )}

      <ButtonRow>
        {signedIn ? (
          <Button tone="primary" onClick={onNext}>
            Continue
          </Button>
        ) : (
          <Button onClick={onNext}>Skip</Button>
        )}
      </ButtonRow>
    </>
  );
}
