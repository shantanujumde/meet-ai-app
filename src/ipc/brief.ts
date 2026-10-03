/**
 * The pre-meeting brief (TUR-32): last time's notes and the commits since.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { hasBackend, narrow } from "./client";
import type { MeetingBrief } from "./types";

/** Last time's notes and the commits since, for the meeting called `title` (TUR-32). */
export async function meetingBrief(title: string): Promise<MeetingBrief> {
  if (!hasBackend()) return { title, previous: null, commits: null };
  return narrow(() => commands.meetingBrief(title));
}
