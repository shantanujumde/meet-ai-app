/**
 * When notes run after a call (TUR-101): `agent.auto_run` in `config.jsonc`.
 * True (the default) starts them once the transcript is final; false waits
 * for "Make notes now" on the meeting.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call, hasBackend } from "./client";

/** On unless `config.jsonc` turns it off. */
export const DEFAULT_NOTES_AUTO_RUN = true;

/** The setting as saved. */
export async function notesAutoRun(): Promise<boolean> {
  if (!hasBackend()) return DEFAULT_NOTES_AUTO_RUN;
  return call(() => commands.notesAutoRun());
}

/** Save the setting. Resolves to what was saved. */
export function saveNotesAutoRun(on: boolean): Promise<boolean> {
  return call(() => commands.saveNotesAutoRun(on));
}
