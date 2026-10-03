/**
 * Settings: which speech engine this Mac will use, the models it can
 * download, which agent writes the notes, and where meetings are written.
 *
 * The agent card is {@link AgentSetup}, shared with onboarding's last step.
 *
 * The engine card is {@link EngineSummary}, shared with onboarding's speech
 * step. It never waits on the ~160 ms engine probe before painting — see its
 * own comment for why getting that backwards makes a settings screen feel slow
 * for a reason the user can never see.
 */

import { useAppStore } from "@/state/app";
import { AudioRetentionRow } from "@/ui/AudioRetentionRow";
import { AgentSetup } from "@/ui/agent/AgentSetup";
import { DockSetting } from "@/ui/DockSetting";
import { EngineSummary } from "@/ui/engine/EngineSummary";
import { FolderRow } from "@/ui/FolderRow";
import { LogsFolderRow } from "@/ui/LogsFolderRow";
import { Button, Card, Row, RowLabel } from "@/ui/primitives";
import { ErrorState } from "@/ui/states";
import { TrackerSettings } from "@/ui/TrackerSettings";

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

      <AgentSetup />

      <section className="section" aria-labelledby="files-heading">
        <h2 className="section__title" id="files-heading">
          Files
        </h2>
        <Card flush>
          <FolderRow status={rootExists ? "Exists" : "Created on first recording"} />
          <AudioRetentionRow />
          <Row>
            <RowLabel
              name="Setup"
              detail="Walk through permission, speech and agent setup again"
              mono={false}
            />
            <Button size="small" onClick={() => void restartOnboarding()}>
              Show setup again
            </Button>
          </Row>
          <LogsFolderRow />
        </Card>
        {onboardingError ? (
          <ErrorState error={onboardingError} onRemedy={() => void restartOnboarding()} />
        ) : null}
      </section>

      {/* TUR-76: closing the window keeps meet-ai running in the menu bar. */}
      <section className="section" aria-labelledby="menu-bar-heading">
        <h2 className="section__title" id="menu-bar-heading">
          Menu bar
        </h2>
        <Card flush>
          <DockSetting />
        </Card>
      </section>

      <TrackerSettings />
    </div>
  );
}
