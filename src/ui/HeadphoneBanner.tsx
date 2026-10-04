/**
 * "No headphones" while recording through speakers (TUR-65, SPEC L6).
 *
 * Without headphones the mic hears the other people too, so both tracks hold
 * the same voices and the transcript repeats lines. SPEC L6 says warn, never
 * fix, so this is only a quiet line under the titlebar: neutral, not the
 * warning tint the permission banner uses, and it never blocks or stops
 * anything. Rust decides when to show it (speakers, with
 * `audio.warn_no_headphones` on); an output it cannot tell apart never
 * shows it.
 *
 * Dismiss hides it for this recording. If the user then puts headphones on
 * and later takes them off again, it comes back: that is a new reason to
 * look.
 */

import { HeadphoneOff, X } from "lucide-react";
import { useEffect, useState } from "react";
import { type HeadphoneWarning, headphoneWarning, onHeadphoneWarning } from "@/ipc/client";
import type { RecordingStatus } from "@/ipc/types";
import { Icon } from "./icons";
import { IconButton } from "./primitives";

export const NO_HEADPHONES_TEXT = "No headphones: the transcript may repeat lines";

export function HeadphoneBanner({ status }: { status: RecordingStatus }) {
  const meetingId = status.phase === "recording" ? status.meetingId : null;
  const [warning, setWarning] = useState<HeadphoneWarning | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    return onHeadphoneWarning((next) => {
      setWarning(next);
      // Headphones went on: a later move back to speakers shows it again.
      if (!next.show) setDismissed(false);
    });
  }, []);

  useEffect(() => {
    setDismissed(false);
    if (meetingId === null) return;
    let cancelled = false;
    // The first reading may have gone out before this window listened.
    headphoneWarning()
      .then((current) => {
        if (cancelled || current === null) return;
        // An event that already arrived for this meeting is newer.
        setWarning((seen) => (seen?.meetingId === current.meetingId ? seen : current));
      })
      .catch(() => {
        // No reading: no banner, which is the right default.
      });
    return () => {
      cancelled = true;
    };
  }, [meetingId]);

  const show = meetingId !== null && warning?.meetingId === meetingId && warning.show && !dismissed;
  if (!show) return null;

  return (
    <div
      role="status"
      className="flex items-center gap-4 border-b-[0.5px] border-separator px-6 py-2 text-footnote text-fg-secondary contrast-more:text-fg-primary"
    >
      <Icon icon={HeadphoneOff} />
      <p className="m-0 flex-1 leading-normal">{NO_HEADPHONES_TEXT}</p>
      <IconButton icon={X} label="Dismiss" onClick={() => setDismissed(true)} />
    </div>
  );
}
