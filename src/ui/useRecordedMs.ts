/**
 * The recording timer as a hook (TUR-146): {@link recordedMs} re-read once a
 * second while recording, and not at all while paused, where it is frozen.
 */

import { useEffect, useState } from "react";
import { recordedMs, type TimerStatus } from "@/lib/elapsed";

export function useRecordedMs(status: TimerStatus): number {
  const [now, setNow] = useState(() => Date.now());
  const running = status.startedAtMs !== null && (status.pause?.pausedAtMs ?? null) === null;

  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [running]);

  return recordedMs(status, now);
}
