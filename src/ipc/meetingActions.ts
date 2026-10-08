/**
 * Changing one meeting from its page or its ⋯ menu: renaming it (TUR-103)
 * and deleting it (TUR-116).
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call } from "./client";

/**
 * Rename a meeting. Answers with the title as written: one line, trimmed and
 * capped. A blank title is refused. The calendar and the agent never replace
 * a name the user gave.
 */
export function renameMeeting(id: string, title: string): Promise<string> {
  return call(() => commands.renameMeeting(id, title));
}

/**
 * Move a meeting's folder to the system Trash: transcript, notes, audio and
 * its own tasks. Nothing is erased, so it can be taken back out of the Trash.
 * Shared tickets that name the meeting stay in Tickets. Refused with kind
 * `recording-in-progress` while the meeting is recording.
 */
export async function deleteMeeting(id: string): Promise<void> {
  await call(() => commands.deleteMeeting(id));
}
