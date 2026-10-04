/**
 * The record button and its elapsed timer.
 *
 * Three rules from the design system are load-bearing here:
 *
 * * **The pulse is never the only signal.** Reduce Motion removes the pulse, so
 *   an elapsed timer sits beside the dot and the button's own words change.
 * * **The record control is solid, never glass**, and it is the one tinted
 *   control on screen while recording.
 * * **It is disabled, not failing at click time**, when permission is denied —
 *   with the reason on the button's accessible name, so a VoiceOver user is not
 *   left wondering why.
 */

import { Mic, Square } from "lucide-react";
import { useEffect, useState } from "react";
import type { PermissionStatus, RecordingStatus } from "@/ipc/types";
import { formatElapsed } from "@/lib/format";
import { osText, shortcutLabel } from "@/lib/osText";
import { recordingBlocked } from "@/lib/recordingPermission";
import { Icon } from "./icons";

/** Human wording for each phase. `Starting`/`Stopping` get their own. */
function labelFor(phase: RecordingStatus["phase"]): string {
  switch (phase) {
    case "idle":
      return "Record";
    case "starting":
      return "Starting…";
    case "recording":
      return "Stop";
    case "stopping":
      return "Stopping…";
  }
}

/**
 * A timer that ticks in the window rather than over IPC.
 *
 * Rust sends the start instant once; sending a tick per second would be an IPC
 * message per second for the entire length of a meeting, to render something
 * the webview can work out itself.
 */
export function useElapsed(startedAtMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (startedAtMs === null) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [startedAtMs]);

  return startedAtMs === null ? 0 : Math.max(0, now - startedAtMs);
}

export function RecordControl({
  status,
  permission,
  busy,
  onToggle,
}: {
  status: RecordingStatus;
  permission: PermissionStatus | null;
  busy: boolean;
  onToggle: () => void;
}) {
  const elapsed = useElapsed(status.startedAtMs);
  const live = status.phase === "recording";
  const transitioning = status.phase === "starting" || status.phase === "stopping";

  // A denied microphone is the only state that disables the control (TUR-87:
  // with only system audio off it records the microphone). "Unknown" must not —
  // it covers both "hasn't checked yet" and "the check itself couldn't run"
  // (no output device, etc.), neither of which is evidence of a denial, and
  // locking the user out on a guess would be worse than letting them try.
  const denied = recordingBlocked(permission);
  const disabled = denied || busy || transitioning;

  const accessibleName = denied
    ? "Recording is unavailable because meet-ai is not allowed to record this Mac's audio"
    : live
      ? `Stop recording. ${formatElapsed(elapsed)} so far.`
      : "Start recording";

  return (
    <div className="record">
      <button
        type="button"
        className="record__button"
        data-recording={live}
        disabled={disabled}
        aria-label={accessibleName}
        title={denied ? `Fix audio permission in ${osText("settings")} first` : shortcutLabel()}
        onClick={onToggle}
      >
        {/* The pulse is the live signal (never the only one: the red fill, the
            elapsed timer and the word Stop carry it too). Idle and the two
            transitions get the action's icon instead. */}
        {live ? (
          <span className="record__dot record__dot--live" aria-hidden="true" />
        ) : (
          <Icon icon={status.phase === "stopping" ? Square : Mic} />
        )}
        {labelFor(status.phase)}
      </button>

      {/* Beside the button it starts, not across the timer's empty room. */}
      <span className="record__shortcut" aria-hidden="true">
        {shortcutLabel()}
      </span>

      {/* Reserved whether or not it is running, so starting a recording does
          not shift the titlebar sideways. Last, so the empty room sits at the
          edge. */}
      <span className="record__elapsed" aria-hidden={!live}>
        {live ? formatElapsed(elapsed) : ""}
      </span>
    </div>
  );
}
