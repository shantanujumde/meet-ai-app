/**
 * "Use the Mac's own mic when Bluetooth headphones are connected" (TUR-91):
 * `audio.use_builtin_mic_with_bluetooth` in `config.jsonc`, on by default.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call, hasBackend } from "./client";

/** On unless `config.jsonc` turns it off. */
export const DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH = true;

/** The setting as saved. */
export async function builtinMicWithBluetooth(): Promise<boolean> {
  if (!hasBackend()) return DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH;
  return call(() => commands.builtinMicWithBluetooth());
}

/** Save the setting. Resolves to what was saved. */
export function setBuiltinMicWithBluetooth(on: boolean): Promise<boolean> {
  return call(() => commands.setBuiltinMicWithBluetooth(on));
}
