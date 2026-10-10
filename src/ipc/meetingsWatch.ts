/**
 * The meetings folder watch problem (TUR-134): when meet-ai cannot watch the
 * folder (Linux's inotify limit, say), changes made in other apps stop
 * showing until a restart. Rust keeps the problem and sends every change.
 *
 * Re-exported from `./client`; import from there.
 */

import {
  commands,
  MEETINGS_WATCH_PROBLEM_EVENT,
  type meet_ai_lib_watch_WatchProblem,
} from "./bindings";
import { call, hasBackend, subscribe } from "./client";

export { MEETINGS_WATCH_PROBLEM_EVENT };

/** What the event carries: the problem, or `null` once watching works again. */
export type WatchProblem = meet_ai_lib_watch_WatchProblem;

/** The current problem; `null` while the folder is watched. */
export async function meetingsWatchProblem(): Promise<string | null> {
  if (!hasBackend()) return null;
  return call(() => commands.meetingsWatchProblem());
}

/** Every change of the problem. */
export function onMeetingsWatchProblem(handler: (problem: WatchProblem) => void): () => void {
  return subscribe(MEETINGS_WATCH_PROBLEM_EVENT, handler);
}
