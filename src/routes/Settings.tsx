/**
 * Settings: which speech engine this Mac will use, the models it can
 * download, and where meetings are written.
 *
 * The engine card is {@link EngineSummary}, shared with onboarding's speech
 * step. It never waits on the ~160 ms engine probe before painting — see its
 * own comment for why getting that backwards makes a settings screen feel slow
 * for a reason the user can never see.
 */

import { useAppStore } from "@/state/app";
import { EngineSummary } from "@/ui/engine/EngineSummary";
import { FolderRow } from "@/ui/FolderRow";
import { ErrorState } from "@/ui/states";

export function Settings() {
  const restartOnboarding = useAppStore((state) => state.restartOnboarding);
  const onboardingError = useAppStore((state) => state.onboardingError);
  const rootExists = useAppStore((state) => state.meetings?.rootExists ?? false);

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="page__title">Settings</h1>
      </header>

      <EngineSummary />

      <section className="section" aria-labelledby="files-heading">
        <h2 className="section__title" id="files-heading">
          Files
        </h2>
        <div className="card card--flush">
          <FolderRow status={rootExists ? "Exists" : "Created on first recording"} />
          <div className="row">
            <span className="row__label">
              <span className="row__name">Setup</span>
              <span className="row__detail" style={{ fontFamily: "var(--font-ui)" }}>
                Walk through permission and speech setup again
              </span>
            </span>
            <button
              type="button"
              className="btn btn--small"
              onClick={() => void restartOnboarding()}
            >
              Show setup again
            </button>
          </div>
        </div>
        {onboardingError ? (
          <ErrorState error={onboardingError} onRemedy={() => void restartOnboarding()} />
        ) : null}
      </section>
    </div>
  );
}
