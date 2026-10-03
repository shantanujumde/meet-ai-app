/**
 * Onboarding, and the screen a user lands on after saying No to audio capture.
 *
 * SPEC §8.1 makes this its own route in v1 rather than a first-launch special
 * case, and makes the denied path a requirement rather than an edge case: one
 * sentence on what breaks, a **Retry** button, and **Open System Settings**
 * deep-linking to Privacy & Security.
 *
 * Two things shape the copy here:
 *
 * * **macOS needs two separate grants.** The microphone carries the `You`
 *   channel and the system audio tap carries `Others` (L5). Granting one and
 *   not the other produces a half-recorded meeting, so both are named.
 * * **"Not checked" is not "denied".** On denial every status code is `noErr`
 *   and the tap returns silence, so only a positive control tone proves
 *   permission (SPEC §8.1) — the backend plays it and listens on every check.
 *   If that check cannot run at all (no output device, a read failure), the
 *   screen still says it does not know, rather than claiming a grant it has
 *   not verified.
 *
 * This file is the wizard itself — which step, the dots, Back and Skip. Each
 * step's copy lives in `onboarding/*Step.tsx`.
 */

import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { calendarSources } from "@/ipc/client";
import {
  isOnboardingStep,
  MEETINGS,
  type OnboardingStep,
  onboardingStepPath,
  onboardingSteps,
} from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { Button, ButtonRow } from "@/ui/primitives";
import { AgentStep } from "./onboarding/AgentStep";
import { CalendarStep } from "./onboarding/CalendarStep";
import { FolderStep } from "./onboarding/FolderStep";
import { PermissionStep } from "./onboarding/PermissionStep";
import { SpeechStep } from "./onboarding/SpeechStep";
import { WelcomeStep } from "./onboarding/WelcomeStep";

export function Onboarding() {
  const { step } = useParams<{ step?: string }>();
  const navigate = useNavigate();
  const current: OnboardingStep = isOnboardingStep(step) ? step : "welcome";
  // TUR-49: off macOS there is a calendar step. A Mac's list until Rust says.
  const [calendarApp, setCalendarApp] = useState(true);
  const STEPS = onboardingSteps(calendarApp);
  const index = STEPS.indexOf(current);

  const permission = useAppStore((state) => state.permission);
  const permissionLoading = useAppStore((state) => state.permissionLoading);
  const loadPermission = useAppStore((state) => state.loadPermission);
  const finish = useAppStore((state) => state.finishOnboarding);

  // Re-check every time this route is opened. Someone who just came back from
  // System Settings must not be shown a cached "denied".
  useEffect(() => {
    void loadPermission();
  }, [loadPermission]);

  useEffect(() => {
    let live = true;
    calendarSources().then(
      (sources) => {
        if (live) setCalendarApp(sources.calendarAppAvailable);
      },
      // Keep the Mac's list: the calendar step is optional anyway.
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);

  function goNext() {
    const next = STEPS[index + 1];
    if (next) navigate(onboardingStepPath(next));
  }

  async function complete() {
    await finish();
    navigate(MEETINGS);
  }

  const previous = STEPS[index - 1];

  return (
    <div className="page page--narrow">
      {/* The dots are decoration; the sentence is the accessible version.
          An aria-label on a plain div is not announced reliably anyway. */}
      <p className="sr-only">
        Step {index + 1} of {STEPS.length}
      </p>
      <div className="flex items-center gap-3" aria-hidden="true">
        {STEPS.map((name, position) => (
          <span
            key={name}
            data-active={position <= index}
            className="size-[6px] rounded-capsule bg-separator-strong data-[active=true]:bg-accent"
          />
        ))}
      </div>

      {current === "welcome" ? <WelcomeStep onNext={goNext} /> : null}
      {current === "permission" ? (
        <PermissionStep
          status={permission}
          loading={permissionLoading}
          onRecheck={() => void loadPermission()}
          onNext={goNext}
        />
      ) : null}
      {current === "speech" ? <SpeechStep onNext={goNext} /> : null}
      {current === "folder" ? <FolderStep onNext={goNext} /> : null}
      {current === "calendar" ? <CalendarStep onNext={goNext} /> : null}
      {current === "agent" ? <AgentStep onFinish={() => void complete()} /> : null}

      <ButtonRow>
        {previous ? (
          <Button tone="quiet" size="small" onClick={() => navigate(onboardingStepPath(previous))}>
            Back
          </Button>
        ) : null}
        <Button tone="quiet" size="small" onClick={() => void complete()}>
          Skip setup
        </Button>
      </ButtonRow>
    </div>
  );
}
