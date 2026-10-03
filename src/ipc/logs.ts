/**
 * The logs folder (TUR-46): `meet-ai.log` and any crash files.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call } from "./client";

/**
 * Open the folder with `meet-ai.log` and any crash files, so the user can
 * attach them to a bug report. Nothing is sent anywhere by the app.
 */
export async function openLogsFolder(): Promise<void> {
  await call(() => commands.openLogsFolder());
}
