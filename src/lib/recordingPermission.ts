/**
 * What an audio-permission answer means for the Record control (TUR-87).
 *
 * The combined state is `denied` when either grant is off, because the
 * permission screen has to name that switch. But only a denied microphone
 * stops a recording: with only System Audio Recording off, meet-ai records
 * your microphone alone (the backend's `Status::blocks_recording` makes the
 * same call), and the window says "System audio is off" instead.
 */

import type { PermissionStatus } from "@/ipc/types";

/** Whether this answer only has System Audio Recording switched off. */
export function systemAudioOnlyOff(permission: PermissionStatus | null): boolean {
  return (
    permission?.state === "denied" &&
    permission.denied.length === 1 &&
    permission.denied[0] === "audio-capture"
  );
}

/** Whether Record must stay disabled: a denial that includes the microphone. */
export function recordingBlocked(permission: PermissionStatus | null): boolean {
  return permission?.state === "denied" && !systemAudioOnlyOff(permission);
}
