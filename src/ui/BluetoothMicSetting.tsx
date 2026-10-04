/**
 * "Use the Mac's own mic when Bluetooth headphones are connected" (TUR-91):
 * `audio.use_builtin_mic_with_bluetooth` in `config.jsonc`.
 *
 * Recording a Bluetooth headset's mic switches the headphones to call mode,
 * which makes everything the user hears quieter. On (the default), meet-ai
 * records the Mac's own mic instead. The switch itself is
 * {@link SettingSwitch}.
 */

import { builtinMicWithBluetooth, setBuiltinMicWithBluetooth } from "@/ipc/client";
import { SettingSwitch } from "./SettingSwitch";

export const BLUETOOTH_MIC_LABEL = "Use the Mac's own mic when Bluetooth headphones are connected";

export function BluetoothMicSetting() {
  return (
    <SettingSwitch
      label={BLUETOOTH_MIC_LABEL}
      detail="Keeps your headphones sounding normal while you record. Turn off to record the headphones' mic."
      load={builtinMicWithBluetooth}
      save={setBuiltinMicWithBluetooth}
    />
  );
}
