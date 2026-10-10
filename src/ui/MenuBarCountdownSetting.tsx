/**
 * "Show next meeting in the menu bar" (TUR-77): `app.menu_bar_countdown` in
 * `config.jsonc`.
 *
 * On, the next meeting's countdown sits next to the menu-bar icon from an
 * hour before it starts ("Weekly sync in 12m"). Off by default: the icon
 * alone. macOS only shows it; a Windows or Linux tray has no room for text.
 * The switch itself is {@link SettingSwitch}.
 */

import { CalendarClock } from "lucide-react";
import { menuBarCountdown, type SettingsSnapshot, setMenuBarCountdown } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";
import { appSettingSaved } from "./settings/AppConfigProblem";
import { useSnapshotLoad } from "./settings/snapshot";

export const MENU_BAR_COUNTDOWN_LABEL = "Show next meeting in the menu bar";

const save = async (on: boolean) => {
  const saved = await setMenuBarCountdown(on);
  appSettingSaved();
  return saved;
};

const pick = (snapshot: SettingsSnapshot) => snapshot.menuBarCountdown;

export function MenuBarCountdownSetting() {
  const load = useSnapshotLoad(pick, menuBarCountdown);
  return (
    <SettingSwitch
      icon={CalendarClock}
      label={MENU_BAR_COUNTDOWN_LABEL}
      detail="From an hour before it starts, like “Weekly sync in 12m”, next to the menu bar icon."
      load={load}
      save={save}
    />
  );
}
