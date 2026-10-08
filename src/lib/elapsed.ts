/**
 * How long a recording has recorded, paused stretches left out (TUR-146).
 *
 * Rust sends the start instant and the pause clock once per change; the
 * window works the rest out itself rather than being sent a tick a second.
 * Pure, so the timer is tested without a clock.
 */

import type { RecordingStatus } from "@/ipc/types";

/** The parts of a status the timer reads. */
export type TimerStatus = Pick<RecordingStatus, "startedAtMs" | "pause">;

/**
 * Milliseconds recorded at `nowMs`: the time since the start, less every
 * finished pause, and frozen where the current pause began. `0` before a
 * start, and never negative (a clock that moved back).
 */
export function recordedMs(status: TimerStatus, nowMs: number): number {
  if (status.startedAtMs === null) return 0;
  const end = status.pause?.pausedAtMs ?? nowMs;
  const paused = status.pause?.pausedTotalMs ?? 0;
  return Math.max(0, end - status.startedAtMs - paused);
}

/** `mm:ss`, or `h:mm:ss` from an hour on: the overlay's timer. */
export function formatTimer(milliseconds: number): string {
  const total = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${pad(minutes)}:${pad(seconds)}`;
}
