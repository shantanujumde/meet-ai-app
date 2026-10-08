/**
 * Pause and resume (TUR-146). Rust keeps the phase `recording` through a
 * pause, stops both channels so nothing is written or transcribed, and puts
 * the paused stretches on the status (`pause`) so every window's timer leaves
 * them out. Re-exported from `./client`; import from there.
 */

import { commands, type meet_ai_lib_recording_pause_PauseClock } from "./bindings";
import { narrow } from "./client";
import type { RecordingStatus } from "./types";

/** The paused stretches of a recording, on `RecordingStatus.pause`. */
export type PauseClock = meet_ai_lib_recording_pause_PauseClock;

/** Never paused: what a status without a `pause` means. */
export const NEVER_PAUSED: PauseClock = { pausedAtMs: null, pausedTotalMs: 0 };

// Pure, so it lives with the timer and a test's `@/ipc/client` mock can use
// the real one without importing this module (which imports `./client`).
export { isPaused } from "@/lib/elapsed";

/** Pause the live recording. Resolves to the status after it. */
export function pauseRecording(): Promise<RecordingStatus> {
  return narrow(() => commands.pauseRecording());
}

/** Carry on recording into the same meeting. Resolves to the status after it. */
export function resumeRecording(): Promise<RecordingStatus> {
  return narrow(() => commands.resumeRecording());
}
