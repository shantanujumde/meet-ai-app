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

import { useEffect } from "react";
import { useNavigate, useParams } from "react-router";
import { MEETINGS, onboardingStepPath } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { FolderStep } from "./onboarding/FolderStep";
import { PermissionStep } from "./onboarding/PermissionStep";
import { SpeechStep } from "./onboarding/SpeechStep";
import { WelcomeStep } from "./onboarding/WelcomeStep";

const STEPS = ["welcome", "permission", "speech", "folder"] as const;
type Step = (typeof STEPS)[number];

function isStep(value: string | undefined): value is Step {
  return STEPS.includes(value as Step);
}

export function Onboarding() {
  const { step } = useParams<{ step?: string }>();
  const navigate = useNavigate();
  const current: Step = isStep(step) ? step : "welcome";
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
      <div className="progress-dots" aria-hidden="true">
        {STEPS.map((name, position) => (
          <span key={name} data-active={position <= index} />
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
      {current === "folder" ? <FolderStep onFinish={() => void complete()} /> : null}

      <div className="btn-row">
        {previous ? (
          <button
            type="button"
            className="btn btn--quiet btn--small"
            onClick={() => navigate(onboardingStepPath(previous))}
          >
            Back
          </button>
        ) : null}
        <button type="button" className="btn btn--quiet btn--small" onClick={() => void complete()}>
          Skip setup
        </button>
      </div>
    </div>
  );
}
