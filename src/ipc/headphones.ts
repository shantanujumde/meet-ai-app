/**
 * The "No headphones" warning while recording (TUR-65, SPEC L6): Rust reads
 * the default output when a recording starts and whenever it changes, and
 * says whether the recording window should warn. Speakers warn; headphones,
 * a headset or an output it cannot tell apart never do.
 *
 * Re-exported from `./client`; import from there.
 */

import {
  commands,
  HEADPHONE_WARNING_EVENT,
  type meet_ai_lib_headphone_warning_HeadphoneWarning as HeadphoneWarning,
} from "./bindings";
import { call, hasBackend, subscribe } from "./client";

export type { HeadphoneWarning };
export { HEADPHONE_WARNING_EVENT };

/** The warning for the recording in progress; `null` before the first read or when idle. */
export async function headphoneWarning(): Promise<HeadphoneWarning | null> {
  if (!hasBackend()) return null;
  return call(() => commands.headphoneWarning());
}

/** Every change of the warning, for whichever meeting is recording: filter by `meetingId`. */
export function onHeadphoneWarning(handler: (warning: HeadphoneWarning) => void): () => void {
  return subscribe<HeadphoneWarning>(HEADPHONE_WARNING_EVENT, handler);
}
