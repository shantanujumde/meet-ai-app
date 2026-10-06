/**
 * Under the Meetings page header when meet-ai cannot watch the meetings folder
 * (TUR-134). Without it, edits made in other apps would silently stop showing.
 * Rust sends every change; the first value is pulled in case it went out
 * before this page listened.
 */

import { useEffect, useState } from "react";
import { meetingsWatchProblem, onMeetingsWatchProblem } from "@/ipc/client";

export const WATCH_PROBLEM_TEXT =
  "meet-ai cannot watch the meetings folder, so changes made in other apps show up after a restart.";

export function WatchProblemNote() {
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let heard = false;
    const stop = onMeetingsWatchProblem((problem) => {
      heard = true;
      setMessage(problem.message);
    });
    meetingsWatchProblem()
      .then((current) => {
        // An event that already arrived is newer.
        if (!cancelled && !heard) setMessage(current);
      })
      .catch(() => {
        // No reading: no note.
      });
    return () => {
      cancelled = true;
      stop();
    };
  }, []);

  if (message === null) return null;
  return (
    <div role="status" className="text-footnote text-fg-secondary wrap-anywhere">
      <p>{WATCH_PROBLEM_TEXT}</p>
      <p>{message}</p>
    </div>
  );
}
