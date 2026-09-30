/**
 * Which recording phase changes the meeting list cares about.
 *
 * Rust moves `idle → starting → recording → stopping → idle`. Only two of
 * those steps touch the meetings folder: the meeting's folder exists once
 * `recording` is reached, and it is finished once the phase is back at
 * `idle`. Re-reading the list on the other steps re-read every WAV header in
 * the root for nothing.
 */

import type { RecordingPhase } from "@/ipc/types";

/**
 * Whether going from `from` to `to` adds a meeting folder or finishes one: a
 * recording that has begun (`idle → recording`, usually through `starting`),
 * or one that has finished writing (`stopping → idle`, or straight from
 * `recording` when Rust ends it on its own — TUR-97).
 */
export function changesMeetingList(from: RecordingPhase, to: RecordingPhase): boolean {
  if (to === "recording") return from === "idle" || from === "starting";
  if (to === "idle") return from === "stopping" || from === "recording";
  return false;
}
