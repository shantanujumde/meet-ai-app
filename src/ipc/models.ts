/**
 * Deleting a downloaded whisper model from Settings → Speech (TUR-132).
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call } from "./client";

/**
 * Delete a downloaded whisper model. Refused with `model-in-use` while it is
 * the picked model or a recording is running.
 */
export function deleteModel(id: string): Promise<void> {
  return call(() => commands.deleteModel(id)).then(() => undefined);
}
