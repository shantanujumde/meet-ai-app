/**
 * Deleting a downloaded whisper model from Settings → Speech (TUR-132), and
 * cancelling a download (TUR-159).
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

/**
 * Stop the download of `id` (TUR-159). The pending `downloadModel` then
 * rejects with `model`/`cancelled`, and the partial file is kept for the next
 * download to resume. Resolves false when `id` was not downloading.
 */
export function cancelModelDownload(id: string): Promise<boolean> {
  return call(() => commands.cancelModelDownload(id));
}
