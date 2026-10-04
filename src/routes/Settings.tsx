/**
 * Settings: which speech engine this Mac will use, the models it can
 * download, the audio settings, which agent writes the notes, and where meetings are written.
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
import { NotesWhenSetting } from "@/ui/agent/NotesWhenSetting";
import { BluetoothMicSetting } from "@/ui/BluetoothMicSetting";
import { CalendarSettings } from "@/ui/calendar/CalendarSettings";
import { DockSetting } from "@/ui/DockSetting";
import { EngineSummary } from "@/ui/engine/EngineSummary";
import { FolderRow } from "@/ui/FolderRow";
import { LogsFolderRow } from "@/ui/LogsFolderRow";
import { MenuBarCountdownSetting } from "@/ui/MenuBarCountdownSetting";
import { NotificationSettings } from "@/ui/NotificationSettings";
import { Button, Card, Row, RowLabel } from "@/ui/primitives";
import { StartAtLoginSetting } from "@/ui/StartAtLoginSetting";
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

      {/* TUR-93: the audio settings, in their own section. */}
      <section className="section" aria-labelledby="audio-heading">
        <h2 className="section__title" id="audio-heading">
          Audio
        </h2>
        <Card flush>
          <AudioRetentionRow />
          {/* TUR-91: keep Bluetooth headphones out of call mode. */}
          <BluetoothMicSetting />
        </Card>
      </section>

      <AgentSetup />
      {/* TUR-101: notes after the call, or only from "Make notes now". */}
      <NotesWhenSetting />

      {/* TUR-49: where meetings come from. */}
      <CalendarSettings />

      <section className="section" aria-labelledby="files-heading">
        <h2 className="section__title" id="files-heading">
          Files
        </h2>
        <Card flush>
          <FolderRow status={rootExists ? "Exists" : "Created on first recording"} />
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
          {/* TUR-77: the next meeting's countdown next to the icon. */}
          <MenuBarCountdownSetting />
          {/* TUR-58: open meet-ai when you log in. Off by default. */}
          <StartAtLoginSetting />
        </Card>
      </section>

      {/* TUR-78: reminders, their lead time, and which prompts ask. */}
      <NotificationSettings />

      <TrackerSettings />
    </div>
  );
}
