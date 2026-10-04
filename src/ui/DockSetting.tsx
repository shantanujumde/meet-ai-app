/**
 * "Show in Dock when the window is closed" (TUR-76): the `app` section's
 * `app.show_in_dock_when_closed` in `config.jsonc`.
 *
 * Closing the window never quits meet-ai: it keeps running in the menu bar
 * so reminders, detection and a recording carry on. Off (the default), the
 * Dock icon goes away with the window, like Granola; on, it stays. The
 * switch itself is {@link SettingSwitch}.
 */

import { Dock } from "lucide-react";
import { appSettings, setShowInDockWhenClosed } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";

export const DOCK_SETTING_LABEL = "Show in Dock when the window is closed";

const load = async () => (await appSettings()).showInDockWhenClosed;
const save = async (on: boolean) => (await setShowInDockWhenClosed(on)).showInDockWhenClosed;

export function DockSetting() {
  return (
    <SettingSwitch
      icon={Dock}
      label={DOCK_SETTING_LABEL}
      detail="Closing the window keeps meet-ai running in the menu bar. Quit from the menu bar icon or with ⌘Q."
      load={load}
      save={save}
    />
  );
}
