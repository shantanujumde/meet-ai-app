/**
 * Renaming a meeting from its page (TUR-103).
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
