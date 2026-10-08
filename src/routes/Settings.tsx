/**
 * Settings: which speech engine this Mac will use, the models it can
 * download, the audio settings, which agent writes the notes, and where meetings are written.
 *
 * The agent card is {@link AgentSetup}, shared with onboarding's last step.
 *
 * TUR-102: every section is a {@link SettingsSection} (a heading over one
 * rounded card) and every row the one row pattern, starting with Appearance.
 *
 * The engine card is {@link EngineSummary}, shared with onboarding's speech
 * step. It never waits on the ~160 ms engine probe before painting — see its
 * own comment for why getting that backwards makes a settings screen feel slow
 * for a reason the user can never see.
 */

import { ListRestart } from "lucide-react";
import { useEffect } from "react";
import { useSearchParams } from "react-router";
import { useAppStore } from "@/state/app";
import { AboutSettings } from "@/ui/AboutSettings";
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
import { OverlaySetting } from "@/ui/OverlaySetting";
import { Button } from "@/ui/primitives";
import { StartAtLoginSetting } from "@/ui/StartAtLoginSetting";
import { AppearanceSettings } from "@/ui/settings/AppearanceSettings";
import { SettingsRow, SettingsSection } from "@/ui/settings/SettingsSection";
import { ErrorState } from "@/ui/states";
import { TrackerSettings } from "@/ui/TrackerSettings";

export function Settings() {
  // TUR-113: `/settings?section=tracker` ("Open Tracker settings") scrolls there.
  const [params] = useSearchParams();
  const section = params.get("section");
  useEffect(() => {
    if (section) document.getElementById(section)?.scrollIntoView({ block: "start" });
  }, [section]);
  const restartOnboarding = useAppStore((state) => state.restartOnboarding);
  const onboardingError = useAppStore((state) => state.onboardingError);
  const rootExists = useAppStore((state) => state.meetings?.rootExists ?? false);

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="page__title">Settings</h1>
      </header>

      {/* TUR-102: Light / Dark / System, and the see-through glass. */}
      <AppearanceSettings />

      <EngineSummary />

      {/* TUR-93: the audio settings, in their own section. */}
      <SettingsSection title="Audio">
        <AudioRetentionRow />
        {/* TUR-91: keep Bluetooth headphones out of call mode. */}
        <BluetoothMicSetting />
        {/* TUR-146: the small always-on-top window while recording. */}
        <OverlaySetting />
      </SettingsSection>

      <AgentSetup />
      {/* TUR-101: notes after the call, or only from "Make notes now". */}
      <NotesWhenSetting />

      {/* TUR-49: where meetings come from. */}
      <CalendarSettings />

      <SettingsSection
        title="Files"
        after={
          onboardingError ? (
            <ErrorState error={onboardingError} onRemedy={() => void restartOnboarding()} />
          ) : null
        }
      >
        <FolderRow status={rootExists ? "Exists" : "Created on first recording"} />
        <SettingsRow
          icon={ListRestart}
          name="Setup"
          detail="Walk through permission, speech and agent setup again"
          control={
            <Button size="small" onClick={() => void restartOnboarding()}>
              Show setup again
            </Button>
          }
        />
        <LogsFolderRow />
      </SettingsSection>

      {/* TUR-76: closing the window keeps meet-ai running in the menu bar. */}
      <SettingsSection title="Menu bar">
        <DockSetting />
        {/* TUR-77: the next meeting's countdown next to the icon. */}
        <MenuBarCountdownSetting />
        {/* TUR-58: open meet-ai when you log in. Off by default. */}
        <StartAtLoginSetting />
      </SettingsSection>

      {/* TUR-78: reminders, their lead time, and which prompts ask. */}
      <NotificationSettings />

      <TrackerSettings />

      {/* TUR-62: the credit Parakeet's CC-BY-4.0 licence asks for. */}
      <AboutSettings />
    </div>
  );
}
