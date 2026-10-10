/**
 * What an audio-permission answer means for the Record control (TUR-87).
 *
 * The combined state is `denied` when either grant is off, because the
 * permission screen has to name that switch. But only a denied microphone
 * stops a recording: with only System Audio Recording off, meet-ai records
 * your microphone alone (the backend's `Status::blocks_recording` makes the
 * same call), and the window says "System audio is off" instead.
 */

import type { PermissionStatus, RecordingStatus } from "@/ipc/types";
import { type Os, osText } from "./osText";

/**
 * The banner line when only System Audio Recording is off. During a
 * recording the check has just found it (TUR-136) and the recording has
 * dropped its system track, so the line says this recording goes on with
 * the microphone; otherwise it says what the next recording will capture.
 */
export function systemAudioOffText(recording: boolean): string {
  const lead = `System audio is off: System Audio Recording is turned off for meet-ai in ${osText("settings")}`;
  return recording
    ? `${lead}, so this recording goes on with your microphone only.`
    : `${lead}, so recordings capture only your microphone.`;
}

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

/**
 * Whether a Record or Stop control is disabled (TUR-170): a denied
 * microphone, a request from this window in flight, or Rust mid-way through
 * a start or a stop. One rule for every such control, so a click during
 * `starting` (after ⌘⇧R) cannot stop the recording that is starting.
 */
export function recordDisabled(
  status: RecordingStatus,
  permission: PermissionStatus | null,
  busy: boolean,
): boolean {
  const transitioning = status.phase === "starting" || status.phase === "stopping";
  return recordingBlocked(permission) || busy || transitioning;
}

/**
 * Whether onboarding shows the permission step on `os` (TUR-51). macOS
 * always does: it guards two grants that must be asked for. Windows only when
 * the microphone is blocked, since loopback needs no permission and an
 * allowed microphone needs no setup. Linux never: it has no audio
 * permissions, and a capture failure there is a device error.
 */
export function permissionStepShown(os: Os, permission: PermissionStatus | null): boolean {
  if (os === "macos") return true;
  if (os === "windows") return recordingBlocked(permission);
  return false;
}
