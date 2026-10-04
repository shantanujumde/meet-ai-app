/**
 * "Start at login" (TUR-58), through `tauri-plugin-autostart` in Rust.
 *
 * Off by default. The OS keeps the setting (a login item), not
 * `config.jsonc`. Starting at login only opens meet-ai; it never starts a
 * recording. The switch itself is {@link SettingSwitch}.
 */

import { setStartAtLogin, startAtLogin } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";

export const START_AT_LOGIN_LABEL = "Start at login";

export function StartAtLoginSetting() {
  return (
    <SettingSwitch
      label={START_AT_LOGIN_LABEL}
      detail="Open meet-ai when you log in. It does not start recording."
      load={startAtLogin}
      save={setStartAtLogin}
    />
  );
}
