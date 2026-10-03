/**
 * "Show next meeting in the menu bar" (TUR-77): `app.menu_bar_countdown` in
 * `config.jsonc`.
 *
 * On, the next meeting's countdown sits next to the menu-bar icon from an
 * hour before it starts ("Weekly sync in 12m"). Off by default: the icon
 * alone. macOS only shows it; a Windows or Linux tray has no room for text.
 * The switch itself is {@link SettingSwitch}.
 */

import { menuBarCountdown, setMenuBarCountdown } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";

export const MENU_BAR_COUNTDOWN_LABEL = "Show next meeting in the menu bar";

export function MenuBarCountdownSetting() {
  return (
    <SettingSwitch
      label={MENU_BAR_COUNTDOWN_LABEL}
      detail="From an hour before it starts, like “Weekly sync in 12m”, next to the menu bar icon."
      load={menuBarCountdown}
      save={setMenuBarCountdown}
    />
  );
}
