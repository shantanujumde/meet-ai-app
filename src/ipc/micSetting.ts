/**
 * "Use the Mac's own mic when Bluetooth headphones are connected" (TUR-91):
 * `audio.use_builtin_mic_with_bluetooth` in `config.jsonc`, on by default.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands, DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH as RUST_DEFAULT } from "./bindings";
import { call, hasBackend } from "./client";

/** On unless `config.jsonc` turns it off. From Rust (TUR-173). */
export const DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH: boolean = RUST_DEFAULT;

/** The setting as saved. */
export async function builtinMicWithBluetooth(): Promise<boolean> {
  if (!hasBackend()) return DEFAULT_BUILTIN_MIC_WITH_BLUETOOTH;
  return call(() => commands.builtinMicWithBluetooth());
}

/** Save the setting. Resolves to what was saved. */
export function setBuiltinMicWithBluetooth(on: boolean): Promise<boolean> {
  return call(() => commands.setBuiltinMicWithBluetooth(on));
}
